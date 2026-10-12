Texture2D HDRSource : register(t0);
// Ambient-таргет главного прохода (слот AMBIENT_SRV_SLOT в той же таблице,
// что и у tonemap): альфа = 1 у unlit-пикселей — они в bloom не идут.
Texture2D AmbientSource : register(t4);
SamplerState PointSampler : register(s0);

cbuffer BloomParams : register(b0) {
    float threshold;
    float2 texel_size; // 1/width, 1/height ИСТОЧНИКА (для blur-прохода; extract его не использует)
    float _unused;
};

struct PS_INPUT {
    float4 pos : SV_POSITION;
    float2 uv : TEXCOORD0;
};

float4 main(PS_INPUT input) : SV_TARGET {
    float3 color = HDRSource.Sample(PointSampler, input.uv).rgb;
    float brightness = max(color.r, max(color.g, color.b));
    // smoothstep(threshold, threshold*2, brightness) — мягкий, а не
    // жёсткий порог: пиксели чуть ниже threshold не пропадают резко
    // в 0, а плавно затухают, что убирает "рваный" край вокруг
    // светящихся объектов после последующего блюра.
    float contribution = smoothstep(threshold, threshold * 2.0, brightness);
    contribution *= 1.0 - saturate(AmbientSource.Sample(PointSampler, input.uv).a);
    return float4(color * contribution, 1.0);
}
