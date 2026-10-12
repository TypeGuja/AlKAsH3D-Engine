// ДОБАВЛЕНО (тени листвы, порт My Summer Car): пиксельный шейдер теневого
// прохода ТОЛЬКО для мешей с альфа-вырезом (Mesh::alpha_cutoff > 0 —
// листва, решётки, трава). Без него тень отбрасывал весь квад листа:
// кроны были сплошными чёрными пятнами и затеняли сами себя. Остальные
// меши рисуются прежним depth-only PSO без PS (ранний тест глубины).
Texture2D AlbedoMap : register(t0);
SamplerState MaterialSampler : register(s0);

cbuffer CutoutConstants : register(b1) {
    float alphaCutoff;
};

struct PS_INPUT {
    float4 pos : SV_POSITION;
    float2 uv : TEXCOORD0;
};

void main(PS_INPUT input) {
    if (AlbedoMap.Sample(MaterialSampler, input.uv).a < alphaCutoff) {
        discard;
    }
}
