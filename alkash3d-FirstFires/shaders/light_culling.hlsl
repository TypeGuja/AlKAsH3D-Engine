// shaders/light_culling.hlsl
// Compute shader для GPU light culling

struct GPULight {
    float4 position;  // xyz, type (0=point,1=spot,2=directional)
    float4 color;     // rgb, intensity
    float4 direction; // xyz, range
    float4 params;    // spot_angle, falloff, lod, padding
};

struct LightGridCell {
    uint offset;
    uint count;
};

struct LightGridEntry {
    uint light_index;
    uint lod_level;
    float depth;
    uint padding;
};

StructuredBuffer<GPULight> InputLights : register(t0);
RWStructuredBuffer<LightGridCell> LightGridCells : register(u0);
RWStructuredBuffer<uint> LightGridIndices : register(u1);
RWStructuredBuffer<uint> VisibleLightCount : register(u2);

cbuffer CullingParams : register(b0) {
    float4x4 ViewProj;
    float3 CameraPosition;
    float FarPlane;
    float3 LODDistances;
    uint MaxLights;
    float CellSize;
    float3 WorldMin;
    float3 WorldMax;
    uint GridWidth;
    uint GridHeight;
    uint GridDepth;
};

[numthreads(64, 1, 1)]
void CullLightsCS(uint3 threadId : SV_DispatchThreadID) {
    uint lightIdx = threadId.x;
    if (lightIdx >= MaxLights) return;

    GPULight light = InputLights[lightIdx];

    // Расстояние до камеры
    float3 toLight = light.position.xyz - CameraPosition;
    float distance = length(toLight);

    // LOD
    uint lod = 0;
    if (distance > LODDistances.x) lod++;
    if (distance > LODDistances.y) lod++;
    if (distance > LODDistances.z) return;  // Выключен

    // Frustum culling
    float4 clipPos = mul(float4(light.position.xyz, 1.0), ViewProj);
    float3 ndc = clipPos.xyz / clipPos.w;
    if (abs(ndc.x) > 1.0 || abs(ndc.y) > 1.0 || ndc.z < 0.0 || ndc.z > 1.0) {
        return;
    }

    // Добавление в grid
    uint3 gridPos;
    gridPos.x = (light.position.x - WorldMin.x) / CellSize;
    gridPos.y = (light.position.y - WorldMin.y) / CellSize;
    gridPos.z = (light.position.z - WorldMin.z) / CellSize;

    if (gridPos.x < GridWidth && gridPos.y < GridHeight && gridPos.z < GridDepth) {
        uint cellIdx = (gridPos.z * GridHeight * GridWidth) +
                       (gridPos.y * GridWidth) +
                       gridPos.x;

        uint listIdx = InterlockedAdd(LightGridCells[cellIdx].count, 1);
        uint entryIdx = LightGridCells[cellIdx].offset + listIdx;

        LightGridIndices[entryIdx] = lightIdx;

        // Отмечаем видимость
        InterlockedAdd(VisibleLightCount[0], 1);
    }
}