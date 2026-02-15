// deferred_light_frag.hlsl
// Считывает G‑buffer и вычисляет освещение для всех активных источников

// ---------- G‑buffer ----------
Texture2D gPos      : register(t0);   // позиция (float4)
Texture2D gNorm     : register(t1);   // нормаль (packed)
Texture2D gAlbedo   : register(t2);   // альбедо
Texture2D gMat      : register(t3);   // материал (metallic, rough, ao)
SamplerState sLinear : register(s0);

// ---------- Списки источников ----------
static const uint MAX_LIGHTS = 256;
struct Light
{
    int    type;        // 0 = directional, 1 = point, 2 = spot
    float3 color;
    float  intensity;
    float3 direction;   // для directional / spot
    float3 position;     // для point / spot
    float  radius;      // для point
    float3 spotDir;     // для spot
    float  innerCutoff; // cos(theta_inner)
    float  outerCutoff; // cos(theta_outer)
};
cbuffer LightCB : register(b0)
{
    int uNumLights;
    Light lights[MAX_LIGHTS];
};

cbuffer CameraCB : register(b1)
{
    float3 camPos;
    float  pad0;
};

float3 DecodeNormal(float4 packed)
{
    // Преобразуем из [0,1] обратно в [-1,1]
    return normalize(packed.xyz * 2.0 - 1.0);
}

// ----------------- Основная функция -----------------
float4 PSMain(float2 uv : TEXCOORD0) : SV_Target
{
    // 1) Считываем G‑buffer
    float3 worldPos = gPos.Sample(sLinear, uv).xyz;
    float3 normal   = DecodeNormal(gNorm.Sample(sLinear, uv));
    float4 albedo   = gAlbedo.Sample(sLinear, uv);
    float4 material = gMat.Sample(sLinear, uv);   // R=metallic, G=rough, B=ao

    float metallic = saturate(material.r);
    float rough    = saturate(material.g);
    float ao       = saturate(material.b);

    // 2) PBR‑базовые расчёты
    float3 N = normalize(normal);
    float3 V = normalize(camPos - worldPos);
    float3 F0 = lerp(float3(0.04, 0.04, 0.04), albedo.rgb, metallic);

    float3 Lo = float3(0,0,0); // итоговый свет

    // 3) Перебираем все источники
    [unroll]
    for (int i = 0; i < uNumLights; ++i)
    {
        Light L = lights[i];
        float3 Ldir;   // направление от точки к свету
        float3 Lcolor = L.color * L.intensity;

        if (L.type == 0)               // directional
        {
            Ldir = normalize(-L.direction);
        }
        else if (L.type == 1)          // point
        {
            Ldir = normalize(L.position - worldPos);
            // attenuation (simple inverse‑square)
            float dist = length(L.position - worldPos);
            float att = saturate(1.0 - dist / L.radius);
            Lcolor *= att;
        }
        else                           // spot
        {
            Ldir = normalize(L.position - worldPos);
            float3 spotDir = normalize(L.spotDir);
            float cosTheta = dot(Ldir, spotDir);
            float spotAtt = smoothstep(L.outerCutoff, L.innerCutoff, cosTheta);
            Lcolor *= spotAtt;
        }

        // ---- Diffuse & specular (Cook‑Torrance) ----
        float NdotL = saturate(dot(N, Ldir));
        if (NdotL > 0.0)
        {
            // halfway vector
            float3 H = normalize(Ldir + V);
            float NdotH = saturate(dot(N, H));
            float VdotH = saturate(dot(V, H));

            // Distribution GGX
            float a = rough * rough;
            float a2 = a * a;
            float NdotH2 = NdotH * NdotH;
            float denom = (NdotH2 * (a2 - 1.0) + 1.0);
            float D = a2 / (PI * denom * denom + 1e-7);

            // Geometry (Smith)
            float k = (rough + 1.0) * (rough + 1.0) / 8.0; // Schlick‑GGX
            float G_Smith = NdotL / (NdotL * (1.0 - k) + k) *
                           NdotV / (NdotV * (1.0 - k) + k);

            // Fresnel (Schlick)
            float3 F = F0 + (1.0 - F0) * pow(1.0 - VdotH, 5.0);

            float3 spec = (D * G_Smith * F) / (4.0 * NdotL * NdotV + 1e-7);
            float3 diff = (1.0 - F) * (1.0 - metallic) * albedo.rgb / PI;

            Lo += (diff + spec) * Lcolor * NdotL;
        }
    }

    // 4) Добавляем ambient term (AO)
    float3 ambient = float3(0.03,0.03,0.03) * albedo.rgb * ao;
    float3 color = ambient + Lo;

    // 5) HDR → LDR (simple tonemap)
    color = color / (color + float3(1.0,1.0,1.0));
    return float4(pow(color, float3(1.0/2.2)), 1.0);
}
