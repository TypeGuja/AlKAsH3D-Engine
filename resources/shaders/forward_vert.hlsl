struct VSInput
{
    float3 position : POSITION;
    float3 normal : NORMAL;
    float2 texcoord : TEXCOORD;
};

struct VSOutput
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

VSOutput main(VSInput input)
{
    VSOutput output;
    
    float4 worldPos = mul(float4(input.position, 1.0), model);
    output.position = mul(worldPos, viewProjection);
    output.worldPos = worldPos.xyz;
    
    output.normal = mul(input.normal, (float3x3)modelInverseTranspose);
    output.texcoord = input.texcoord;
    
    return output;
}