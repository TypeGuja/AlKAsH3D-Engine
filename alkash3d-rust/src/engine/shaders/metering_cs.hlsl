// Замер экспозиции, как у камеры: средняя логарифмическая яркость кадра
// (до экспозиции) с весом к центру. Одна группа: каждый поток берёт свою
// часть сетки выборок, сумма — через groupshared.

Texture2D<float4> HdrTex : register(t0);
RWByteAddressBuffer Result : register(u0);

cbuffer MeteringConstants : register(b0) {
    float2 size;
    float2 _pad;
};

#define THREADS 256
#define GRID_X 64
#define GRID_Y 36

groupshared float sSum[THREADS];
groupshared float sW[THREADS];

[numthreads(THREADS, 1, 1)]
void main(uint3 id : SV_GroupThreadID) {
    float sum = 0.0;
    float wsum = 0.0;
    for (uint i = id.x; i < GRID_X * GRID_Y; i += THREADS) {
        float2 uv = (float2(i % GRID_X, i / GRID_X) + 0.5) / float2(GRID_X, GRID_Y);
        int2 p = int2(uv * size);
        float3 c = HdrTex.Load(int3(p, 0)).rgb;
        float lum = dot(c, float3(0.2126, 0.7152, 0.0722));
        if (!(lum == lum) || lum > 1e6) {
            continue;
        }
        // центр важнее краёв (как центровзвешенный замер)
        float2 d = (uv - 0.5) * float2(1.6, 1.0);
        float w = exp(-dot(d, d) * 4.0);
        sum += log(max(lum, 1e-5)) * w;
        wsum += w;
    }
    sSum[id.x] = sum;
    sW[id.x] = wsum;
    GroupMemoryBarrierWithGroupSync();
    for (uint s = THREADS / 2; s > 0; s >>= 1) {
        if (id.x < s) {
            sSum[id.x] += sSum[id.x + s];
            sW[id.x] += sW[id.x + s];
        }
        GroupMemoryBarrierWithGroupSync();
    }
    if (id.x == 0) {
        Result.Store(0, asuint(exp(sSum[0] / max(sW[0], 1e-6))));
    }
}
