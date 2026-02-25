struct PSInput
{
    float4 position : SV_POSITION;
    float3 normal : NORMAL;
    float2 texcoord : TEXCOORD;
    float3 worldPos : WORLDPOS;
};

cbuffer PerFrame : register(b0)
{
    float4x4 view;
    float4x4 projection;
    float4x4 viewProjection;
    float4 cameraPos;
    float4 lightDir;
    float4 lightColor;
    float4 ambientColor;
};

cbuffer PerObject : register(b1)
{
    float4x4 model;
    float4x4 modelInverseTranspose;
};

Texture2D albedoTexture : register(t0);
SamplerState albedoSampler : register(s0);

float4 main(PSInput input) : SV_Target
{
    // Базовый оранжевый цвет
    float4 baseColor = float4(1.0, 0.5, 0.2, 1.0);
    
    // Простое освещение
    float3 normal = normalize(input.normal);
    float3 lightDirNorm = normalize(-lightDir.xyz);
    float diff = max(dot(normal, lightDirNorm), 0.0);
    
    float4 result = baseColor * (ambientColor + diff * lightColor);
    result.a = 1.0;
    
    return result;
}