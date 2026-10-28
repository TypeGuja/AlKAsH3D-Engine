// ДОБАВЛЕНО (bloom цепочкой уровней): уровень k+1 -> k, вдвое больше.
// Результат ПРИБАВЛЯЕТСЯ к уровню k (PSO со смешиванием ONE/ONE), так что
// к верхнему уровню приходит сумма всех: узкое ядро плюс всё более широкие
// ореолы — гладкий спад, как у рассеяния в линзе/глазу, без "квадратов".
Texture2D BloomSource : register(t0);
SamplerState LinearSampler : register(s0);

cbuffer BloomParams : register(b0) {
    float2 texel_size; // 1/width, 1/height ИСТОЧНИКА (меньшего уровня)
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

// Тент-фильтр 3x3 (веса 1-2-1) билинейными выборками.
float4 main(PS_INPUT input) : SV_TARGET {
    float2 t = texel_size;
    float2 uv = input.uv;
    float3 result = S(uv) * 4.0;
    result += (S(uv + t * float2(0, -1)) + S(uv + t * float2(-1, 0)) + S(uv + t * float2(1, 0)) + S(uv + t * float2(0, 1))) * 2.0;
    result += S(uv + t * float2(-1, -1)) + S(uv + t * float2(1, -1)) + S(uv + t * float2(-1, 1)) + S(uv + t * float2(1, 1));
    return float4(result / 16.0, 1.0);
}
