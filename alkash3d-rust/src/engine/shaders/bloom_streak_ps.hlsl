// ДОБАВЛЕНО (лучи от ламп, AlkashEngine::lens.streaks): свет, рассеянный на
// параллельных царапинах стекла перед объективом, — прямая через источник, в
// обе стороны от него. Читает копию выделенного яркого (уровень 0 bloom),
// результат прибавляется к самому уровню 0 (PSO со смешиванием ONE/ONE): на
// половинном разрешении луч тонкий, как на снимке.
Texture2D BloomSource : register(t0);
SamplerState LinearSampler : register(s0);

cbuffer StreakParams : register(b0) {
    float2 reach;   // конец луча относительно источника, в uv
    float strength; // доля света источника, ушедшая в луч
    float fromDist; // луч появляется у источников дальше fromDist метров
    float toDist;   // и в полную силу — дальше toDist (0 — у всех сразу)
    float farGain;  // показатель усиления с расстоянием
};

struct PS_INPUT {
    float4 pos : SV_POSITION;
    float2 uv : TEXCOORD0;
};

float3 Source(float2 uv) {
    float4 c = BloomSource.SampleLevel(LinearSampler, uv, 0);
    // альфа уровня 0 — яркость на расстояние (см. bloom_extract_ps)
    float lum = dot(c.rgb, float3(0.2126, 0.7152, 0.0722));
    float dist = c.a / max(lum, 1e-5);
    // дальняя лампа — пара пикселей, и света в луч попадает мало; на снимках
    // лучи у дальних ламп не слабее (подобрано по снимкам, это не физика)
    float k = pow(max(dist / 10.0, 1.0), farGain);
    return c.rgb * k * (toDist > 0.0 ? smoothstep(fromDist, toDist, dist) : 1.0);
}

float4 main(PS_INPUT input) : SV_TARGET {
    float3 sum = Source(input.uv);
    float total = 1.0;
    // шаг не шире 5 пикселей кадра, иначе луч от мелкой лампы идёт пунктиром
    for (int i = 1; i <= 32; i++) {
        float t = (float)i / 33.0;
        // у источника ярко, к концу плавно сходит на нет
        float w = (1.0 - t) * (1.0 - t);
        float2 d = reach * t;
        sum += (Source(input.uv + d) + Source(input.uv - d)) * w;
        total += 2.0 * w;
    }
    return float4(sum / total * strength, 1.0);
}
