// Подписи углов на слое ANGLE: в каждом кадре поворота — свой угол (3D-сборка копировала их с ключей).
// FIXA = [[файл, символ, [начало Turn180, начало Turn180B]], ...]
function gcd(a, b) { return b ? gcd(b, a % b) : a; }
function label(deg) {
    var e = Math.round(deg * 8), w = Math.floor(e / 8), r = e % 8, s = deg.toFixed(3) + "° = ";
    if (!r) return s + w + "°";
    var g = gcd(r, 8);
    return s + (w ? w + " " : "") + (r / g) + "/" + (8 / g) + "°";
}
for (var q = 0; q < FIXA.length; q++) {
    var doc = openDoc(FIXA[q][0]);
    try { doc.exitEditMode(); } catch (e) {}
    doc.library.editItem(findItem(doc, FIXA[q][1]).name);
    var tl = doc.getTimeline(), n = 0;
    for (var s = 0; s < FIXA[q][2].length; s++) for (var k = 0; k < 33; k++) {
        var F = FIXA[q][2][s] + k;
        for (var li = 0; li < tl.layers.length; li++) {
            var L = tl.layers[li]; if (L.name != "ANGLE" || !L.frames[F]) continue;
            var fr = L.frames[F]; if (fr.startFrame != F) continue;
            for (var e = 0; e < fr.elements.length; e++) if (fr.elements[e].elementType == "text") {
                var lk = L.locked; L.locked = false; fr.elements[e].setTextString(label(k * 5.625)); L.locked = lk; n++;
            }
        }
    }
    doc.exitEditMode(); fl.saveDocument(doc); log(FIXA[q][0].split("/").pop() + ": подписей " + n);
}
