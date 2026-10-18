// Новый ракурс: копия ключа base в кадр N (все слои), позиции на t к ключу other (тот же слой и символ),
// firstFrame по ff {символ: кадр} (массив — по порядку встреч).
function makeView(doc, sym, base, other, N, t, ff) {
    doc.library.editItem(findItem(doc, sym).name);
    var tl = doc.getTimeline();
    if (tl.frameCount < N + 2) tl.insertFrames(N + 2 - tl.frameCount, true, tl.frameCount - 1);
    for (var i = 0; i < tl.layers.length; i++) {
        var L = tl.layers[i]; if (L.layerType == "folder") continue;
        var lk = L.locked; L.locked = false;
        tl.setSelectedLayers(i); tl.currentLayer = i;
        if (!L.frames[N]) { L.locked = lk; continue; }
        if (L.frames[N].startFrame != N) tl.convertToBlankKeyframes(N);
        if (L.frames[N + 1] && L.frames[N + 1].startFrame != N + 1) tl.convertToBlankKeyframes(N + 1);
        tl.copyFrames(base, base + 1); tl.pasteFrames(N, N + 1);
        L.locked = lk;
    }
    var moved = 0, notes = [], seen = {};
    for (var i = 0; i < tl.layers.length; i++) {
        var L = tl.layers[i]; if (L.layerType == "folder") continue;
        var fN = L.frames[N], fO = L.frames[other]; if (!fN) continue;
        for (var k = 0; k < fN.elements.length; k++) {
            var e = fN.elements[k]; if (e.elementType != "instance") continue;
            var nm = e.libraryItem.name.split("/").pop();
            if (fO && L.layerType != "guide") for (var q = 0; q < fO.elements.length; q++) {
                var o = fO.elements[q];
                if (o.elementType == "instance" && o.libraryItem == e.libraryItem) {
                    var m = e.matrix; m.tx += (o.matrix.tx - m.tx) * t; m.ty += (o.matrix.ty - m.ty) * t; e.matrix = m; moved++; break;
                }
            }
            if (ff[nm] !== undefined) {
                var v = ff[nm]; if (v instanceof Array) { seen[nm] = (seen[nm] || 0); v = v[Math.min(seen[nm]++, v.length - 1)]; }
                e.firstFrame = v; notes.push(L.name + ":" + nm + "=" + v);
            }
        }
    }
    doc.exitEditMode();
    return "сдвинуто " + moved + "; " + notes.join(", ");
}
