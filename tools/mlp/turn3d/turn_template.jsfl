// Полуоборот 0..180° с шагом 5.625° (33 ракурса, метка "Turn180") для любых пони DHX.
// JOB подставляет make_turn_job.py: { src, dst, render, ponies: [{ char, head, keys: [[кадр, угол]...],
//   tail3fix: bool, recolor: { символ: { кадр: { "#OLD": "#NEW" } } } }] }.
// Ракурс — твин частей между соседними ключами; рисунок — с ближайшего ключа; детали без
// пары едут жёстко за якорем (торс/череп), волосы — за головой. Брови из "эмоциональных"
// кадров (ключ+1) — на guide-слои CLAUDE_BROW_n головы.
var JOB = /*JOB*/null;
var STEP = 5.625, COUNT = 33;

function gcd(a, b) { return b ? gcd(b, a % b) : a; }
function label(deg) {
    var e = Math.round(deg * 8), w = Math.floor(e / 8), r = e % 8, s = deg.toFixed(3) + "° = ";
    if (!r) return s + w + "°";
    var g = gcd(r, 8);
    return s + (w ? w + " " : "") + (r / g) + "/" + (8 / g) + "°";
}
function fam(n) { return n.replace(/_(7-8|3-4|5-8|front|side|back|3-4rear|3-4far)$/i, "").replace(/\(overlap\)$/, ""); }
function lerpM(a, b, t) { var m = a; m.a += (b.a - a.a) * t; m.b += (b.b - a.b) * t; m.c += (b.c - a.c) * t; m.d += (b.d - a.d) * t; m.tx += (b.tx - a.tx) * t; m.ty += (b.ty - a.ty) * t; return m; }
function comp(A, B) {
    return { a: A.a * B.a + A.c * B.b, b: A.b * B.a + A.d * B.b, c: A.a * B.c + A.c * B.d, d: A.b * B.c + A.d * B.d,
             tx: A.a * B.tx + A.c * B.ty + A.tx, ty: A.b * B.tx + A.d * B.ty + A.ty };
}
function inv(M) {
    var k = M.a * M.d - M.b * M.c, a = M.d / k, b = -M.b / k, c = -M.c / k, d = M.a / k;
    return { a: a, b: b, c: c, d: d, tx: -(a * M.tx + c * M.ty), ty: -(b * M.tx + d * M.ty) };
}
function setM(e, M) { var m = e.matrix; m.a = M.a; m.b = M.b; m.c = M.c; m.d = M.d; m.tx = M.tx; m.ty = M.ty; e.matrix = m; }
function plain(m) { return { a: m.a, b: m.b, c: m.c, d: m.d, tx: m.tx, ty: m.ty }; }
var ANCHOR = /(_Torso|_Skull|_Body)$/i, RIGID = /(neck|chest|overlap|cutie)/i;
function inst(fr) { var r = []; if (!fr) return r; for (var i = 0; i < fr.elements.length; i++) if (fr.elements[i].elementType == "instance") r.push(fr.elements[i]); return r; }
function skipLayer(L) { var n = L.name.toUpperCase(); return L.layerType == "folder" || n == "AUDIO" || n.indexOf("NOT FINAL") >= 0; }

function makePlan(KEYS) {
    var plan = [];
    // Без твина (JOB.noTween): каждый кадр — точная копия ближайшего нарисованного ракурса.
    if (typeof JOB != "undefined" && JOB.noTween) {
        for (var k = 0; k < COUNT; k++) {
            var deg = k * STEP, best = KEYS[0];
            for (var j = 1; j < KEYS.length; j++) if (Math.abs(KEYS[j][1] - deg) < Math.abs(best[1] - deg)) best = KEYS[j];
            plan.push({ deg: deg, src: best[0], other: best[0], t: 0 });
        }
        return plan;
    }
    for (var k = 0; k < COUNT; k++) {
        var deg = k * STEP, j = 0;
        while (j < KEYS.length - 2 && KEYS[j + 1][1] <= deg) j++;
        var a = KEYS[j], b = KEYS[j + 1], t = (deg - a[1]) / (b[1] - a[1]);
        if (t >= 1 - 1e-9) plan.push({ deg: deg, src: b[0], other: b[0], t: 0 });
        else if (t < 0.5) plan.push({ deg: deg, src: a[0], other: b[0], t: t });
        else plan.push({ deg: deg, src: b[0], other: a[0], t: 1 - t });
    }
    return plan;
}

// Дотянуть таймлайн символа до S0 кадров (персонаж и голова — синхронно).
function extendTo(symName, S0) {
    doc.library.editItem(findItem(doc, symName).name);
    var tl = doc.getTimeline();
    if (tl.frameCount < S0) tl.insertFrames(S0 - tl.frameCount, true, tl.frameCount);
    doc.exitEditMode();
}

var HEAD_NAME = null;
var RIGID_TWEEN = (typeof JOB != "undefined" && JOB.rigidTween) || false;
var MANE_FOLLOWS_HEAD = (typeof JOB != "undefined" && JOB.maneFollowsHead) || false;
function build(symName, byLayer, plan, S0) {
    doc.library.editItem(findItem(doc, symName).name);
    var tl = doc.getTimeline();
    if (tl.frameCount != S0) throw new Error(symName + ": кадров " + tl.frameCount + ", ожидалось " + S0);
    tl.insertFrames(COUNT + 2, true, S0);
    var moved = 0, lost = {};
    log(symName + ": копирую кадры");
    for (var i = 0; i < tl.layers.length; i++) {
        var L = tl.layers[i];
        if (L.layerType == "folder") continue;
        var lk = L.locked; L.locked = false;
        tl.setSelectedLayers(i); tl.currentLayer = i;
        for (var k = 0; k < COUNT; k++) {
            var F = S0 + k;
            if (skipLayer(L)) { tl.convertToBlankKeyframes(F); continue; }
            tl.copyFrames(plan[k].src, plan[k].src + 1);
            tl.pasteFrames(F, F + 1);
        }
        tl.convertToBlankKeyframes(S0 + COUNT);
        L.locked = lk;
    }
    log(symName + ": твин");
    for (var k = 0; k < COUNT; k++) {
        var p = plan[k], F = S0 + k;
        if (p.t == 0) continue;
        var pool = [], used = [], items = [];
        if (!byLayer) for (var i = 0; i < tl.layers.length; i++) if (!skipLayer(tl.layers[i])) pool = pool.concat(inst(tl.layers[i].frames[p.other]));
        for (var i = 0; i < tl.layers.length; i++) {
            var L = tl.layers[i];
            if (skipLayer(L)) continue;
            var els = inst(L.frames[F]), cand = byLayer ? inst(L.frames[p.other]) : pool;
            for (var q = 0; q < els.length; q++) {
                var e = els[q], nm = e.libraryItem.name, best = -1, same = false;
                for (var j = 0; j < cand.length; j++) if (used.indexOf(cand[j]) < 0 && cand[j].libraryItem.name == nm) { best = j; same = true; break; }
                if (best < 0) for (var j = 0; j < cand.length; j++) if (used.indexOf(cand[j]) < 0 && fam(cand[j].libraryItem.name) == fam(nm)) { best = j; break; }
                if (best >= 0) used.push(cand[best]); else lost[nm.split("/").pop()] = 1;
                items.push({ e: e, L: L, o: best >= 0 ? cand[best] : null, same: same, nm: nm });
            }
        }
        var an = null;
        for (var q = 0; q < items.length; q++) if (items[q].same && ANCHOR.test(items[q].nm)) { an = items[q]; break; }
        if (!an) for (var q = 0; q < items.length; q++) if (items[q].same && (!an || items[q].e.width * items[q].e.height > an.e.width * an.e.height)) an = items[q];
        var D = null, DH = null;
        if (an) D = comp(lerpM(plain(an.e.matrix), plain(an.o.matrix), p.t), inv(plain(an.e.matrix)));
        for (var q = 0; q < items.length; q++) if (items[q].same && /_Head$/.test(items[q].nm))
            DH = comp(lerpM(plain(items[q].e.matrix), plain(items[q].o.matrix), p.t), inv(plain(items[q].e.matrix)));
        for (var q = 0; q < items.length; q++) {
            var it = items[q], lk = it.L.locked; it.L.locked = false;
            // Грива/волосы — всегда за головой (иначе куски гривы между ракурсами вылезают из-под волос).
            // Жёсткий твин (редкие ключи): между далёкими ракурсами двигаются только опора и голова
            // целиком, остальное — вместе с ними (иначе одноимённые детали разъезжаются).
            if (RIGID_TWEEN && it !== an && !(it.same && /_Head$/.test(it.nm))) {
                if (DH && /(mane|hair)/i.test(it.nm)) setM(it.e, comp(DH, plain(it.e.matrix)));
                else if (D) setM(it.e, comp(D, plain(it.e.matrix)));
            }
            else if (DH && it !== an && MANE_FOLLOWS_HEAD && /(mane|hair)/i.test(it.nm)) setM(it.e, comp(DH, plain(it.e.matrix)));
            else if (it === an || (it.same && !RIGID.test(it.nm))) setM(it.e, lerpM(plain(it.e.matrix), plain(it.o.matrix), p.t));
            else if (DH && /(mane|hair)/i.test(it.nm)) setM(it.e, comp(DH, plain(it.e.matrix)));
            else if (D) setM(it.e, comp(D, plain(it.e.matrix)));
            it.L.locked = lk;
            moved++;
        }
    }
    var synced = 0;
    for (var k = 0; k < COUNT; k++) {
        var F = S0 + k;
        for (var i = 0; i < tl.layers.length; i++) {
            var fr = tl.layers[i].frames[F];
            if (!fr || fr.startFrame != F) continue;
            for (var q = 0; q < fr.elements.length; q++) {
                var e = fr.elements[q];
                if (e.elementType == "instance" && (e.libraryItem.name == HEAD_NAME || e.firstFrame == plan[k].src) && e.libraryItem.timeline && e.libraryItem.timeline.frameCount > S0) {
                    var lk = tl.layers[i].locked; tl.layers[i].locked = false; e.firstFrame = F; tl.layers[i].locked = lk; synced++;
                }
            }
        }
    }
    var ls = []; for (var n in lost) ls.push(n);
    log("  " + symName + ": твинов " + moved + ", синхр. " + synced + (ls.length ? ", без пары: " + ls.slice(0, 8).join(", ") + (ls.length > 8 ? "…" : "") : ""));
    return tl;
}

function labels(tl, plan, S0, name) {
    for (var i = 0; i < tl.layers.length; i++) if (tl.layers[i].name.toUpperCase() == "AUDIO") {
        tl.setSelectedLayers(i); tl.currentLayer = i;
        tl.convertToBlankKeyframes(S0);
        tl.layers[i].frames[S0].name = name || "Turn180";
        tl.layers[i].frames[S0].labelType = "name";
    }
    tl.currentLayer = 0;
    var li = tl.addNewLayer("ANGLE", "guide", true);
    tl.setSelectedLayers(li); tl.currentLayer = li;
    for (var k = 0; k < COUNT; k++) {
        var F = S0 + k;
        tl.convertToBlankKeyframes(F);
        tl.currentFrame = F;
        doc.addNewText({ left: -180, top: -230, right: 180, bottom: -200 }, label(plan[k].deg));
        var te = tl.layers[li].frames[F].elements[0], n = te.getTextString().length;
        te.setTextAttr("size", 28, 0, n); te.setTextAttr("fillColor", "#D00000", 0, n); te.setTextAttr("bold", true, 0, n); te.setTextAttr("face", "Arial", 0, n);
        te.autoExpand = true; te.x = -150; te.y = -235;
        doc.selectNone();
    }
    tl.layers[li].layerType = "guide";
}

function tail3fix(tl, plan, S0) {
    var fixed = 0;
    for (var k = 0; k < COUNT; k++) { var F = S0 + k;
        for (var i = 0; i < tl.layers.length; i++) { var fr = tl.layers[i].frames[F]; if (!fr || fr.startFrame != F) continue;
            for (var q = 0; q < fr.elements.length; q++) { var e = fr.elements[q];
                if (e.elementType == "instance" && /_Tail$/.test(e.libraryItem.name) && e.firstFrame == 3) {
                    var lk = tl.layers[i].locked; tl.layers[i].locked = false; e.firstFrame = plan[k].deg < 90 ? 0 : 2; tl.layers[i].locked = lk; fixed++; } } } }
    log("  хвостов исправлено: " + fixed);
}

function brows(head, plan, S0) {
    doc.library.editItem(findItem(doc, head).name);
    var tl = doc.getTimeline(), idx = [], made = 0;
    for (var i = 0; i < tl.layers.length; i++) if (tl.layers[i].name == "BROW") idx.push(i);
    for (var bi = idx.length - 1; bi >= 0; bi--) {
        tl.currentLayer = idx[bi];
        var gi = tl.addNewLayer("CLAUDE_BROW_" + bi, "normal", true), bl = gi + 1;
        var G = tl.layers[gi], B = tl.layers[bl];
        for (var k = 0; k < COUNT; k++) {
            var p = plan[k], F = S0 + k, s1 = p.src + 1, o1 = p.other + 1;
            tl.setSelectedLayers(gi); tl.currentLayer = gi;
            tl.convertToBlankKeyframes(F);
            if (!inst(B.frames[s1]).length) continue;
            var lk = B.locked; B.locked = false;
            tl.setSelectedLayers(bl); tl.currentLayer = bl;
            tl.copyFrames(s1, s1 + 1);
            B.locked = lk;
            tl.setSelectedLayers(gi); tl.currentLayer = gi;
            tl.pasteFrames(F, F + 1);
            var ne = inst(G.frames[F]), oe = inst(B.frames[o1]);
            if (p.t > 0 && ne.length && oe.length && ne[0].libraryItem.name == oe[0].libraryItem.name) ne[0].matrix = lerpM(ne[0].matrix, oe[0].matrix, p.t);
            made++;
        }
        tl.convertToBlankKeyframes(S0 + COUNT);
        G.layerType = "guide";
    }
    doc.exitEditMode();
    log("  брови: кадров " + made);
}

function recolorAll(spec) {
    for (var sym in spec) for (var f in spec[sym]) log("  перекраска " + sym + " кадр " + f + ": " + recolor(doc, sym, parseInt(f, 10), spec[sym][f]));
}

var doc = openCopy(JOB.src, JOB.dst);
log("копия открыта: " + JOB.dst);
for (var pi = 0; pi < JOB.ponies.length; pi++) {
    var P = JOB.ponies[pi], plan = makePlan(P.keys), hplan = makePlan(P.headKeys || P.keys);
    HEAD_NAME = findItem(doc, P.head).name;
    var S0 = Math.max(findItem(doc, P.char).timeline.frameCount, findItem(doc, P.head).timeline.frameCount);
    log(P.char + ": S0=" + S0);
    extendTo(P.char, S0);
    extendTo(P.head, S0);
    build(P.head, true, hplan, S0);
    var tl = build(P.char, false, plan, S0);
    if (P.tail3fix) tail3fix(tl, plan, S0);
    labels(tl, plan, S0, P.label);
    doc.exitEditMode();
    if (P.recolor) recolorAll(P.recolor);
    if (!P.noBrows) brows(P.head, hplan, S0);
    fl.saveDocument(doc);
    if (JOB.render) renderSymbol(doc, P.char, S0 + 1, S0 + COUNT, JOB.render + "/" + P.char.replace(/[^\w]/g, "_") + (P.label ? "_" + P.label : ""));
    log(P.char + " готов");
}
log("готово: " + JOB.dst);
