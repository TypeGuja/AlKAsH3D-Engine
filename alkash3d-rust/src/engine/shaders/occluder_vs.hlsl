cbuffer OccluderConstants : register(b0) {
    float4x4 viewProj;
};

struct VS_INPUT {
    float3 unitCubePos : POSITION;
    float3 instanceMin : INSTANCE_MIN;
    float3 instanceMax : INSTANCE_MAX;
};
struct VS_OUTPUT {
    float4 pos : SV_POSITION;
};
VS_OUTPUT main(VS_INPUT input) {
    VS_OUTPUT output;
    float3 t = input.unitCubePos * 0.5 + 0.5;
    float3 worldPos = lerp(input.instanceMin, input.instanceMax, t);
    output.pos = mul(viewProj, float4(worldPos, 1.0));
    return output;
}
