Texture2D HDRSource : register(t0);
// Ambient-таргет главного прохода (слот AMBIENT_SRV_SLOT в той же таблице,
// что и у tonemap): альфа = 1 у unlit-пикселей — они в bloom не идут.
Texture2D AmbientSource : register(t4);
SamplerState LinearSampler : register(s0);

// ИЗМЕНЕНО (bloom цепочкой уровней): параметры — root constants, а не общий
// constant buffer (тот перезаписывался всеми проходами кадра до исполнения,
// и GPU видел у всех последний — оба "разделимых" блюра шли по вертикали).
cbuffer BloomParams : register(b0) {
    float2 texel_size; // 1/width, 1/height ИСТОЧНИКА (полноразмерного HDR)
    float threshold;
    float scale;       // доля энергии на уровень (1 / число уровней)
};

struct PS_INPUT {
    float4 pos : SV_POSITION;
    float2 uv : TEXCOORD0;
};

// Яркая часть пикселя: smoothstep(threshold, threshold*2, brightness) —
// мягкий, а не жёсткий порог, без "рваного" края вокруг светящихся объектов.
float3 Prefilter(float2 uv) {
    float3 color = HDRSource.Sample(LinearSampler, uv).rgb;
    float brightness = max(color.r, max(color.g, color.b));
    float contribution = smoothstep(threshold, threshold * 2.0, brightness);
    contribution *= 1.0 - saturate(AmbientSource.Sample(LinearSampler, uv).a);
    return color * contribution;
}

// Понижение в 2 раза 13 билинейными выборками (Jimenez, "Next Generation
// Post Processing in Call of Duty: Advanced Warfare", 2014): усреднение без
// дрожания тонких ярких деталей при движении камеры.
float4 main(PS_INPUT input) : SV_TARGET {
    float2 t = texel_size;
    float2 uv = input.uv;
    float3 a = Prefilter(uv + t * float2(-2, -2));
    float3 b = Prefilter(uv + t * float2( 0, -2));
    float3 c = Prefilter(uv + t * float2( 2, -2));
    float3 d = Prefilter(uv + t * float2(-1, -1));
    float3 e = Prefilter(uv + t * float2( 1, -1));
    float3 f = Prefilter(uv + t * float2(-2,  0));
    float3 g = Prefilter(uv);
    float3 h = Prefilter(uv + t * float2( 2,  0));
    float3 i = Prefilter(uv + t * float2(-1,  1));
    float3 j = Prefilter(uv + t * float2( 1,  1));
    float3 k = Prefilter(uv + t * float2(-2,  2));
    float3 l = Prefilter(uv + t * float2( 0,  2));
    float3 m = Prefilter(uv + t * float2( 2,  2));
    float3 result = (d + e + i + j) * 0.125 + g * 0.125 + (b + f + h + l) * 0.0625 + (a + c + k + m) * 0.03125;
    return float4(result * scale, 1.0);
}
