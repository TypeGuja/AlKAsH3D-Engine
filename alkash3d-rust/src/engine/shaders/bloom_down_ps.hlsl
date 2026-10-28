// ДОБАВЛЕНО (bloom цепочкой уровней): уровень k -> k+1, вдвое меньше.
Texture2D BloomSource : register(t0);
SamplerState LinearSampler : register(s0);

cbuffer BloomParams : register(b0) {
    float2 texel_size; // 1/width, 1/height ИСТОЧНИКА
    float threshold;   // не используется
    float scale;       // не используется
};

struct PS_INPUT {
    float4 pos : SV_POSITION;
    float2 uv : TEXCOORD0;
};

float3 S(float2 uv) {
    return BloomSource.Sample(LinearSampler, uv).rgb;
}

// Те же 13 выборок, что и в bloom_extract_ps (Jimenez 2014).
float4 main(PS_INPUT input) : SV_TARGET {
    float2 t = texel_size;
    float2 uv = input.uv;
    float3 a = S(uv + t * float2(-2, -2));
    float3 b = S(uv + t * float2( 0, -2));
    float3 c = S(uv + t * float2( 2, -2));
    float3 d = S(uv + t * float2(-1, -1));
    float3 e = S(uv + t * float2( 1, -1));
    float3 f = S(uv + t * float2(-2,  0));
    float3 g = S(uv);
    float3 h = S(uv + t * float2( 2,  0));
    float3 i = S(uv + t * float2(-1,  1));
    float3 j = S(uv + t * float2( 1,  1));
    float3 k = S(uv + t * float2(-2,  2));
    float3 l = S(uv + t * float2( 0,  2));
    float3 m = S(uv + t * float2( 2,  2));
    float3 result = (d + e + i + j) * 0.125 + g * 0.125 + (b + f + h + l) * 0.0625 + (a + c + k + m) * 0.03125;
    return float4(result, 1.0);
}
