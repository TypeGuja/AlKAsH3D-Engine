cbuffer FrameCB : register(b0)
{
    float4x4 uView;   // 0‑й 4×4‑массив
    float4x4 uProj;   // 1‑й
    float4x4 uModel;  // 2‑й
};

struct VS_IN
{
    float3 pos : POSITION;   // vertex position
    float2 uv  : TEXCOORD0;  // texture coords
};

struct VS_OUT
{
    float4 pos : SV_POSITION; // позиция в экранных координатах
    float2 uv  : TEXCOORD0;    // передаём дальше
};

VS_OUT VSMain(VS_IN i)
{
    VS_OUT o;
    float4 world = mul(uModel, float4(i.pos, 1.0));
    float4 view  = mul(uView,  world);
    o.pos = mul(uProj, view);
    o.uv  = i.uv;
    return o;
}
