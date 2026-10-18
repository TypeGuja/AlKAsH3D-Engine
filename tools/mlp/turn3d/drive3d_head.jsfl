// Голова по 3D: для кадра поворота i — свой кадр головы HEADF+i (копия ключевого), черты лица
// сдвигаются относительно черепа как у 3D-головы. Вызов после drive3d_v2 (нужны MARKS, JOBD).
function headKind(layer, sym) {
    var L = layer.toUpperCase(), s = sym.toUpperCase();
    if (s.indexOf("EAR") >= 0 && s.indexOf("EARRING") < 0) return "ear";
    if (s.indexOf("HORN") >= 0) return "horn";
    if (s.indexOf("CROWN") >= 0) return "crown";
    if (L.indexOf("BLINK") >= 0 || L == "MASK" || L.indexOf("EYEMASK") >= 0 || L.indexOf("HIGHLIGHT") >= 0 || L.indexOf("PUPIL") >= 0 || L.indexOf("IRIS") >= 0 || L.indexOf("EYEWHITE") >= 0) return "eye";
    if (L.indexOf("BROW") >= 0) return "brow";
    if (L.indexOf("NOSTRIL") >= 0) return "nose";
    if (L.indexOf("LIP") >= 0 || L.indexOf("LYP") >= 0 || L.indexOf("MOUTH") >= 0) return "mouth";
    if (L.indexOf("CHIN") >= 0 || L.indexOf("WRINKLE") >= 0) return "base";  // контур черепа
    if (L.indexOf("HAIR") >= 0 || s.indexOf("HAIR") >= 0 || L.indexOf("MANE_CURL") >= 0 || s.indexOf("MANE_CURL") >= 0) return "bangs";
    return "base";
}
function feat3d(M, kind, leftImg) {  // точка черты в 3D; leftImg — элемент в левой половине головы
    var J = M.J, pick = function (base) {
        var a = J["Left" + base], b = J["Right" + base];
        var l = a[0] <= b[0] ? a : b, r = a[0] <= b[0] ? b : a; return leftImg ? l : r;
    };
    if (kind == "eye") return pick("EyeLidUpper");
    if (kind == "brow") return pick("EyeBrow");
    if (kind == "ear") return pick("EarTip");
    if (kind == "nose") return J.Nose;
    if (kind == "mouth") return J.LeftLipUpMiddle;
    if (kind == "horn") return [M.horn.cx, M.horn.cy];
    if (kind == "crown") return [M.crown.cx, M.crown.cy];
    if (kind == "bangs") { var a = J.LeftEyeBrow, b = J.RightEyeBrow; return [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2 + 0.05]; }
    return J.Head;
}
// Мультяшное лицо DHX плоское: 3D-сдвиги черт ослаблены (морда у 3D выступает сильнее).
var HEAD_DAMP = { eye: 0.55, brow: 0.55, nose: 0.35, mouth: 0.35, ear: 0.6, horn: 0.5, crown: 0.5, bangs: 0.5 };
function driveHead(doc, headSym, hk, HF, Mk, Ma, dhxHeadW) {
    doc.library.editItem(findItem(doc, headSym).name);
    var tl = doc.getTimeline();
    if (tl.frameCount < HF + 2) tl.insertFrames(HF + 2 - tl.frameCount, true, tl.frameCount - 1);
    for (var li = 0; li < tl.layers.length; li++) {
        var L = tl.layers[li]; if (L.layerType == "folder" || !L.frames[HF]) continue;
        var lk = L.locked; L.locked = false; tl.setSelectedLayers(li); tl.currentLayer = li;
        if (L.frames[HF].startFrame != HF) tl.convertToBlankKeyframes(HF);
        if (L.frames[HF + 1] && L.frames[HF + 1].startFrame != HF + 1) tl.convertToBlankKeyframes(HF + 1);
        tl.copyFrames(hk, hk); tl.pasteFrames(HF, HF);
        L.locked = lk;
    }
    var sh = dhxHeadW / (Mk.head.x1 - Mk.head.x0), fb = frameBounds(tl, HF), hcx = fb.left + fb.width / 2, n = 0;
    var hK = Mk.J.Head, hA = Ma.J.Head;
    for (var li = 0; li < tl.layers.length; li++) {
        var L = tl.layers[li]; if (L.layerType == "folder" || L.layerType == "guide" || !L.frames[HF]) continue;
        var els = L.frames[HF].elements;
        for (var e = 0; e < els.length; e++) {
            var el = els[e]; if (el.elementType != "instance") continue;
            var kind = headKind(L.name, el.libraryItem.name.split("/").pop()); if (kind == "base") continue;
            var left = el.left + el.width / 2 < hcx, fk = feat3d(Mk, kind, left), fa = feat3d(Ma, kind, left);
            var kd = HEAD_DAMP[kind] || 0.5, dx = ((fa[0] - hA[0]) - (fk[0] - hK[0])) * sh * kd, dy = -((fa[1] - hA[1]) - (fk[1] - hK[1])) * sh * kd;
            var m = el.matrix; m.tx += dx; m.ty += dy; el.matrix = m; n++;
        }
    }
    doc.exitEditMode();
    return n;
}
