cbuffer FrameCB : register(b0)
{
    float4x4 uView;
    float4x4 uProj;
    float4x4 uModel;
    float4   uTint;
};

float4 main() : SV_TARGET
{
    return uTint;
}
