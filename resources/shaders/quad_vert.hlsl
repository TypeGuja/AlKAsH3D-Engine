// quad_vert.hlsl
// Версия «full‑screen triangle», генерируемая только по VertexID

struct VS_OUT
{
    float4 posH : SV_Position;
    float2 uv   : TEXCOORD0;
};

VS_OUT VSMain(uint id : SV_VertexID)
{
    VS_OUT o;
    // Треугольник, покрывающий весь экран
    float2 pos = float2( (id == 1) ?  3.0 : -1.0,
                         (id == 2) ?  3.0 : -1.0 );
    o.posH = float4(pos, 0.0, 1.0);
    // UV в диапазоне [0,1]
    o.uv = (pos * 0.5) + 0.5;
    return o;
}
