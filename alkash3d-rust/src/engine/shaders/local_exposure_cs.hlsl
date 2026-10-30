// Локальная экспозиция: средняя логарифмическая яркость HDR-кадра (до
// экспозиции) по клеткам сетки — поток на клетку, 8x8 выборок в ней.
// Tonemap читает сетку размыто и подтягивает яркость каждого места к
// средней по кадру: глаз одновременно видит и колбу, и тёмный угол, а
// камера с одной экспозицией на кадр — нет.

Texture2D<float4> HdrTex : register(t0);
RWTexture2D<float> Grid : register(u0);

cbuffer LocalExposureConstants : register(b0) {
    float2 size;
    float2 gridSize;
};

[numthreads(8, 8, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    if (id.x >= (uint)gridSize.x || id.y >= (uint)gridSize.y) {
        return;
    }
    float2 cell = size / gridSize;
    float sum = 0.0;
    float n = 0.0;
    [unroll] for (int y = 0; y < 8; y++) {
        [unroll] for (int x = 0; x < 8; x++) {
            int2 p = int2((float2(id.xy) + (float2(x, y) + 0.5) / 8.0) * cell);
            float3 c = HdrTex.Load(int3(p, 0)).rgb;
            float lum = dot(c, float3(0.2126, 0.7152, 0.0722));
            if (lum == lum && lum < 1e6) {
                sum += log(max(lum, 1e-5));
                n += 1.0;
            }
        }
    }
    Grid[id.xy] = n > 0.0 ? sum / n : log(1e-5);
}
