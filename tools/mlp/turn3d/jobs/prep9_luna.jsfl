// Источник с 9 ракурсами: новые кадры головы 140..146, тела 150..160 (A — сложенные крылья, B — раскрытые).
var doc = openCopy("C:/Temp/claude/animate_render/luna_c28/STK_C28_Princess_Luna_view.fla", "C:/Temp/claude/luna3d/STK_C28_Princess_Luna_9views.fla");
var H157 = { "PL_HeadBase": 3, "PL_Crown": 3 };
log("H140 " + makeView(doc, "~PL_Head", 108, 106, 140, 0.5, {}));
log("H142 " + makeView(doc, "~PL_Head", 110, 108, 142, 0.5, H157));
log("H144 " + makeView(doc, "~PL_Head", 109, 107, 144, 0.5, {}));
log("H146 " + makeView(doc, "~PL_Head", 111, 109, 146, 0.5, H157));
log("C150 " + makeView(doc, "~PL_Character", 107, 105, 150, 0.5, { "PL_Neck": 1, "PL_Collar": 5, "PL_Hair_Cycle_A": 1 }));
log("C152 " + makeView(doc, "~PL_Character", 109, 107, 152, 0.5, { "PL_Neck": 3, "PL_Hair_Cycle_A": 3 }));
log("C154 " + makeView(doc, "~PL_Character", 111, 109, 154, 0.5, { "PL_Torso": 4, "PL_Neck": 8 }));
log("C156 " + makeView(doc, "~PL_Character", 106, 104, 156, 0.5, { "PL_Neck": 1, "PL_Collar": 5, "PL_Hair_Cycle_A": 1 }));
log("C158 " + makeView(doc, "~PL_Character", 108, 106, 158, 0.5, { "PL_Neck": 3, "PL_Hair_Cycle_A": 3 }));
log("C160 " + makeView(doc, "~PL_Character", 110, 108, 160, 0.5, { "PL_Torso": 4, "PL_Neck": 8 }));
saveAs(doc, "C:/Temp/claude/luna3d/STK_C28_Princess_Luna_9views.fla");
fl.closeDocument(doc, false);
