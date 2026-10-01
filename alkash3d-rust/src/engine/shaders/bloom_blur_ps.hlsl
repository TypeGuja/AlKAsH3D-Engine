Texture2D BloomSource : register(t0);
SamplerState PointSampler : register(s0);

cbuffer BloomParams : register(b0) {
    float threshold; // не используется в blur-проходе
    float2 texel_size;
    float _unused;
};

struct PS_INPUT {
    float4 pos : SV_POSITION;
    float2 uv : TEXCOORD0;
};

float4 main(PS_INPUT input) : SV_TARGET {
    // Веса 9-тапового биномиального гаусса (сумма = 1.0), центр —
    // самый большой вес, симметрично убывает к краям.
    float weights[5] = { 0.227027, 0.1945946, 0.1216216, 0.054054, 0.016216 };
    float3 result = BloomSource.Sample(PointSampler, input.uv).rgb * weights[0];
    for (int i = 1; i < 5; i++) {
        float2 offset = texel_size * float(i);
        result += BloomSource.Sample(PointSampler, input.uv + offset).rgb * weights[i];
        result += BloomSource.Sample(PointSampler, input.uv - offset).rgb * weights[i];
    }
    return float4(result, 1.0);
}
