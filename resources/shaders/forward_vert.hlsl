cbuffer FrameCB : register(b0)
{
    float4x4 uView;
    float4x4 uProj;
    float4x4 uModel;
    float4   uTint;
};

struct VS_IN
{
    float3 pos : POSITION;
};

struct VS_OUT
{
    float4 pos : SV_POSITION;
};

VS_OUT main(VS_IN i)
{
    VS_OUT o;
    float4 world = mul(uModel, float4(i.pos, 1.0));
    float4 view  = mul(uView,  world);
    o.pos = mul(uProj, view);
    return o;
}
