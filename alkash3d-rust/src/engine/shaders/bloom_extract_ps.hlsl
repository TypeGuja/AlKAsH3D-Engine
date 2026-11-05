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
    float cap;         // предел яркости пикселя (Lens::bloom_cap), 0 — без предела
};

struct PS_INPUT {
    float4 pos : SV_POSITION;
    float2 uv : TEXCOORD0;
};

// Яркая часть пикселя: smoothstep(threshold, threshold*2, brightness) —
// мягкий, а не жёсткий порог, без "рваного" края вокруг светящихся объектов.
// Альфа — яркость на расстояние до источника в метрах (у Mesh::glow, иначе 0):
// проход лучей делит её на яркость и получает расстояние в этом месте.
float4 Prefilter(float2 uv) {
    // нить лампы ярче диапазона float16 и в кадре может быть +INF: одна такая
    // точка залила бы весь bloom
    float3 color = min(HDRSource.Sample(LinearSampler, uv).rgb, 60000.0);
    float brightness = max(color.r, max(color.g, color.b));
    // предел — общим множителем, по каналам он изменил бы оттенок
    if (cap > 0.0 && brightness > cap) {
        color *= cap / brightness;
        brightness = cap;
    }
    float contribution = smoothstep(threshold, threshold * 2.0, brightness);
    float label = AmbientSource.Sample(LinearSampler, uv).a;
    contribution *= 1.0 - saturate(label);
    color *= contribution;
    return float4(color, dot(color, float3(0.2126, 0.7152, 0.0722)) * max(-label, 0.0));
}

// Понижение в 2 раза 13 билинейными выборками (Jimenez, "Next Generation
// Post Processing in Call of Duty: Advanced Warfare", 2014): усреднение без
// дрожания тонких ярких деталей при движении камеры.
float4 main(PS_INPUT input) : SV_TARGET {
    float2 t = texel_size;
    float2 uv = input.uv;
    float4 a = Prefilter(uv + t * float2(-2, -2));
    float4 b = Prefilter(uv + t * float2( 0, -2));
    float4 c = Prefilter(uv + t * float2( 2, -2));
    float4 d = Prefilter(uv + t * float2(-1, -1));
    float4 e = Prefilter(uv + t * float2( 1, -1));
    float4 f = Prefilter(uv + t * float2(-2,  0));
    float4 g = Prefilter(uv);
    float4 h = Prefilter(uv + t * float2( 2,  0));
    float4 i = Prefilter(uv + t * float2(-1,  1));
    float4 j = Prefilter(uv + t * float2( 1,  1));
    float4 k = Prefilter(uv + t * float2(-2,  2));
    float4 l = Prefilter(uv + t * float2( 0,  2));
    float4 m = Prefilter(uv + t * float2( 2,  2));
    float4 result = (d + e + i + j) * 0.125 + g * 0.125 + (b + f + h + l) * 0.0625 + (a + c + k + m) * 0.03125;
    return result * scale;
}
