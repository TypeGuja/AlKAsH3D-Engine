cbuffer ShadowConstants : register(b0) {
    float4x4 modelLightViewProj;
};

struct VS_INPUT {
    float4 pos : POSITION;
    float3 normal : NORMAL;
    float4 color : COLOR;
    float2 uv : TEXCOORD0;
};
struct VS_OUTPUT {
    float4 pos : SV_POSITION;
    // ДОБАВЛЕНО (тени листвы): UV для shadow_ps.hlsl; depth-only PSO его игнорирует.
    float2 uv : TEXCOORD0;
};
VS_OUTPUT main(VS_INPUT input) {
    VS_OUTPUT output;
    output.pos = mul(modelLightViewProj, input.pos);
    output.uv = input.uv;
    return output;
}
