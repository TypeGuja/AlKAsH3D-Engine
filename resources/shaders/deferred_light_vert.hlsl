// deferred_light_vert.hlsl
// Полноэкранный треугольник, генерируемый только по SV_VertexID

struct VS_OUT
{
    float4 posH : SV_Position;  // позиция в клип‑пространстве
    float2 uv   : TEXCOORD0;    // координаты текстуры (0‑1)
};

VS_OUT VSMain(uint id : SV_VertexID)
{
    VS_OUT o;

    // 3‑вершинный треугольник, покрывающий весь экран
    // (–1,–1) → (3,–1) → (–1,3)   →  UV = (0,0) (2,0) (0,2)
    float2 pos = float2( (id == 1) ? 3.0 : -1.0,
                        (id == 2) ? 3.0 : -1.0 );
    o.posH = float4(pos, 0.0, 1.0);

    // Приводим в [0,1] диапазон для семплинга G‑buffer
    o.uv = (pos * 0.5) + 0.5;
    return o;
}
