cbuffer ShadowConstants : register(b0) {
    float4x4 modelLightViewProj;
};

struct VS_INPUT {
    float4 pos : POSITION;
    float3 normal : NORMAL;
    float4 color : COLOR;
};
struct VS_OUTPUT {
    float4 pos : SV_POSITION;
};
VS_OUTPUT main(VS_INPUT input) {
    VS_OUTPUT output;
    output.pos = mul(modelLightViewProj, input.pos);
    return output;
}
