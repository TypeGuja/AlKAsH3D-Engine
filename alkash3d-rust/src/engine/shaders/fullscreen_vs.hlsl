struct VS_OUTPUT {
    float4 pos : SV_POSITION;
    float2 uv : TEXCOORD0;
};
VS_OUTPUT main(uint vertexId : SV_VertexID) {
    VS_OUTPUT output;
    // Классический fullscreen-triangle трюк: 3 вершины покрывают
    // весь [-1,1]x[-1,1] экран одним треугольником (с запасом за
    // пределами экрана, что нормально — растеризатор отсекает
    // невидимую часть). UV идёт от (0,0) в левом верхнем углу до
    // (2,2) в "запасной" вершине, но реально используемая часть —
    // [0,1]x[0,1], как у обычного квада.
    float2 uv = float2((vertexId << 1) & 2, vertexId & 2);
    output.uv = uv;
    output.pos = float4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    return output;
}
