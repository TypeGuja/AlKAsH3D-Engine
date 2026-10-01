struct GPULight {
    float4 position;
    float4 color;
    float4 direction;
    float4 params;
};
StructuredBuffer<GPULight> Lights : register(t0);

// ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): пространственная
// сетка FirstFires — LightGridCell/LightGridEntry зеркалят layout
// Rust-структур в plugin/light_api.rs. GridCells[cellIndex] даёт
// offset+count в GridEntries, GridEntries[offset..offset+count]
// даёт индексы в Lights[] — именно эти фонари реально пересекают
// ДАННУЮ ячейку мира, а не весь видимый список кадра. Раньше (Фаза
// 2) шейдер перебирал ВСЕ lightCount видимых фонарей на КАЖДЫЙ
// пиксель — корректно, но не масштабируется на город с сотнями
// источников. Теперь пиксель проверяет только фонари своей ячейки.
struct LightGridCell {
    uint offset;
    uint count;
};
struct LightGridEntry {
    uint lightIndex;
    uint lodLevel;
    float depth;
    uint padding;
};
StructuredBuffer<LightGridCell> GridCells : register(t1);
StructuredBuffer<LightGridEntry> GridEntries : register(t2);

// ОБНОВЛЕНО (Cascaded Shadow Maps): раньше здесь была ОДНА shadow
// map. Теперь NUM_CASCADES (=3) отдельных текстур t3, t4, t5 —
// HLSL требует объявлять Texture2D массив как ФИКСИРОВАННОЕ число
// именованных регистров (Texture2D ShadowMap[3] тоже возможен
// синтаксически, но неоднородный размер дескрипторной таблицы и
// индексация по переменной внутри массива текстур не везде
// одинаково поддерживаются старым SM 5.0 без динамической
// индексации ресурсов — явные 3 регистра надёжнее и проще). t3
// идёт следом за t0..t2 фонарей/сетки выше, s0 свободен (у этой
// root signature раньше не было ни одного статического сэмплера
// вообще). Порядок регистров ОБЯЗАН совпадать с NumDescriptors/
// BaseShaderRegister в create_root_signature (engine/mod.rs).
Texture2D ShadowMapCascade0 : register(t3);
Texture2D ShadowMapCascade1 : register(t4);
Texture2D ShadowMapCascade2 : register(t5);
SamplerComparisonState ShadowSampler : register(s0);

// ДОБАВЛЕНО (тени фонарей): depth-атласы теней фонарей — spot (t9)
// и point (t10, 6 граней куба на фонарь), и их матрицы (b2).
// GPULight.params.w: +N = spot-плитка N-1; -N = point-фонарь N-1
// (его грани — плитки (N-1)*6 .. (N-1)*6+5); 0 = фонарь без тени.
// Раскладка cbuffer ОБЯЗАНА совпадать с constant_buffer::SpotShadowConstants.
Texture2D SpotShadowAtlas : register(t9);
Texture2D PointShadowAtlas : register(t10);
cbuffer SpotShadowConstants : register(b2) {
    float4x4 spotShadowViewProj[16];
    float4x4 pointShadowViewProj[48];
    float4 spotShadowParams;  // x = плиток в ряду, y = 1/x, z = разрешение плитки
    float4 pointShadowParams; // x, y, z — то же для point-атласа; w = tan(половины угла грани)
};

// ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы): albedo-текстура
// ТЕКУЩЕГО рисуемого меша — register t6 (следующий свободный после
// t3..t5 shadow-каскадов), root-параметр 5, отдельная descriptor
// table, перебиндивается на каждый Draw (см. render_frame). Читается
// через MaterialSampler (register s1 — обычный линейный WRAP-сэмплер,
// ОТДЕЛЬНЫЙ от compare-сэмплера ShadowSampler s0: `Texture2D.Sample`
// с SamplerComparisonState недопустим в HLSL). Меши без собственной
// текстуры получают белую (1,1,1,1) fallback-текстуру в ЭТОМ ЖЕ
// регистре (см. `white_texture_srv_fallback` в render_frame) — PS
// ниже поэтому МОЖЕТ безусловно сэмплировать AlbedoMap для КАЖДОГО
// меша без отдельной HLSL-ветки "текстуры нет вообще".
Texture2D AlbedoMap : register(t6);
SamplerState MaterialSampler : register(s1);

// ДОБАВЛЕНО (Задача #15, normal mapping): normal map (t7) и
// metallic-roughness map (t8) ТЕКУЩЕГО рисуемого меша — root-
// параметры 6 и 7 (отдельные descriptor table, см.
// create_root_signature), тот же MaterialSampler (s1), что и у
// AlbedoMap. Меши без своей карты получают нейтральные fallback-
// текстуры (flat normal (128,128,255) / dummy MR) в ЭТИХ ЖЕ
// регистрах — тот же принцип "безусловное чтение без HLSL-ветки",
// что и у AlbedoMap выше.
Texture2D NormalMap : register(t7);
Texture2D MetallicRoughnessMap : register(t8);

// ДОБАВЛЕНО (Задача #15, normal mapping): root constants (root-
// параметр 8, register b1). `rootMetallic`/`rootRoughness` —
// скалярные PBR-параметры ЭТОГО меша (см. `Mesh::material_metallic`/
// `material_roughness`). `hasMrMap` — явный флаг (1.0/0.0): 1.0
// значит "у меша есть собственная MetallicRoughnessMap, читать её",
// 0.0 значит "карты нет (или fallback-текстура), использовать
// rootMetallic/rootRoughness напрямую". Флаг обязателен: SRV сам по
// себе не несёт признака "это fallback или реальные данные" — это
// знает только Rust-сторона (`Mesh::mr_srv_index.is_some()`, см.
// render_frame), поэтому передаётся явным third root constant'ом,
// а не выводится внутри HLSL.
// ДОБАВЛЕНО (светящиеся плафоны): lightEmitterArea и materialEmissive —
// см. `Mesh::light_emitter_area` / `Mesh::material_emissive` и
// EmitterRadiance ниже. Раскладка ОБЯЗАНА совпадать с массивом из 8
// root constants в render_frame.rs (параметр 8).
cbuffer MaterialConstants : register(b1) {
    float rootMetallic;
    float rootRoughness;
    float hasMrMap;
    float lightEmitterArea;
    float3 materialEmissive;
    float _materialConstantsPadding;
};

cbuffer TransformConstants : register(b0) {
    float4x4 modelViewProj;
    float4x4 model;
    float4x4 view;
    float4x4 proj;
    float4 cameraPos;
    float4 lightDir;
    float4 lightColor;
    float4 ambientColor;
    uint lightCount;
    uint3 _lightCountPadding;
    float4 gridWorldMin; // xyz = world_min, w = cell_size
    uint4 gridDimensions; // x,y,z = grid_width/height/depth
    // ОБНОВЛЕНО (Cascaded Shadow Maps): порядок ОБЯЗАН совпадать с
    // Rust-структурой TransformConstants (constant_buffer.rs) —
    // массив light_view_proj[NUM_CASCADES] идёт СРАЗУ за
    // gridDimensions, как и там, затем cascadeSplitDistances.
    float4x4 lightViewProj[3];
    float4 cascadeSplitDistances; // x,y,z = дальние границы каскадов 0,1,2 (view-space, метры); w не используется
    float shadowBias;
    float shadowMapSize;
    uint shadowsEnabled;
    uint _shadowPadding;
};

struct PS_INPUT {
    float4 pos : SV_POSITION;
    float4 color : COLOR;
    float3 worldPos : TEXCOORD0;
    float3 normal : TEXCOORD1;
    // ДОБАВЛЕНО (Задача #15): см. VS_OUTPUT.uv в вершинном шейдере
    // выше — те же TEXCOORD2, интерполируется растеризатором
    // между вершинами треугольника как обычно.
    float2 uv : TEXCOORD2;
    // ДОБАВЛЕНО (Задача #15, normal mapping): см. VS_OUTPUT.tangent
    // выше — TEXCOORD3.
    float4 tangent : TEXCOORD3;
};

// ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): 3x3 PCF
// (Percentage-Closer Filtering) — вместо ОДНОГО сравнения глубины
// (что дало бы резкий, "лестничный" край тени — ступеньки шириной
// в один тексель shadow map, ЗАМЕТНЫЕ "попы"/дрожание при движении
// камеры, что явно запрещено требованиями проекта) берём 9
// сравнений в радиусе одного текселя вокруг искомой точки и
// усредняем результат — край тени становится плавным градиентом,
// а не жёсткой границей. `SampleCmpLevelZero` — аппаратная
// инструкция сравнения (сравнивает ЗАПИСАННУЮ в shadow map глубину
// с переданной `compareDepth` и возвращает 0..1 результат
// билинейной интерполяции 2x2 соседних сравнений одним вызовом) —
// используем её как строительный блок 3x3 сетки вместо ручной
// проверки каждого текселя по отдельности (что потребовало бы
// Texture2D::Load вместо Sample и было бы медленнее).
// ОБНОВЛЕНО (Cascaded Shadow Maps): PCF теперь принимает индекс
// каскада и сэмплирует СООТВЕТСТВУЮЩУЮ текстуру — статическая
// ветка по cascadeIndex (0/1/2), а не динамическая индексация
// массива текстур (см. комментарий у ShadowMapCascade0..2 выше,
// почему регистры именованные, а не Texture2D[3]).
float SampleShadowPCF(int cascadeIndex, float3 shadowCoord) {
    float texelSize = 1.0 / max(shadowMapSize, 1.0);
    float sum = 0.0;
    [unroll]
    for (int y = -1; y <= 1; y++) {
        [unroll]
        for (int x = -1; x <= 1; x++) {
            float2 offset = float2(x, y) * texelSize;
            float2 uv = shadowCoord.xy + offset;
            if (cascadeIndex == 0) {
                sum += ShadowMapCascade0.SampleCmpLevelZero(ShadowSampler, uv, shadowCoord.z);
            } else if (cascadeIndex == 1) {
                sum += ShadowMapCascade1.SampleCmpLevelZero(ShadowSampler, uv, shadowCoord.z);
            } else {
                sum += ShadowMapCascade2.SampleCmpLevelZero(ShadowSampler, uv, shadowCoord.z);
            }
        }
    }
    return sum / 9.0;
}

// ДОБАВЛЕНО (Cascaded Shadow Maps): выбирает индекс каскада по
// view-space глубине пикселя (расстояние вдоль оси взгляда камеры,
// НЕ euclidean-дистанция до камеры — то же соглашение, что и у
// cascade_far_distances в render_frame/engine/mod.rs, которые
// считаются как camera.far * CASCADE_SPLITS). Берём САМЫЙ БЛИЖНИЙ
// каскад, чья дальняя граница ещё не меньше viewDepth — то есть
// первый каскад, который "накрывает" эту глубину; если пиксель
// дальше самого дальнего каскада (viewDepth > cascadeSplitDistances.z),
// всё равно используем последний каскад, а не отключаем тени
// резко на границе — плавнее деградирует на самом краю дальности
// теней, чем полное отсутствие тени.
int SelectCascade(float viewDepth) {
    if (viewDepth <= cascadeSplitDistances.x) {
        return 0;
    } else if (viewDepth <= cascadeSplitDistances.y) {
        return 1;
    }
    return 2;
}

// Возвращает множитель освещённости directional-света от теней:
// 1.0 = полностью освещён, 0.0 = полностью в тени. Безопасные
// fallback'и на 1.0 (не в тени) — если тени выключены глобально
// (shadowsEnabled==0, например .alfar сцена не загружена) ИЛИ
// пиксель вне ортографического объёма shadow map (координаты shadow
// space вне [0,1] по X/Y или вне [0,1] по Z — за near/far
// light-проекции); последнее НЕ должно происходить для видимой
// геометрии (объём подгоняется под camera frustum, см.
// compute_cascade_view_proj в engine/mod.rs), но защищает от чтения
// границы карты при численных краевых случаях.
//
// ОБНОВЛЕНО (Cascaded Shadow Maps): принимает уже готовый viewDepth
// (view-space Z пикселя, см. вызов в main() ниже) — используется
// ДВАЖДЫ: чтобы выбрать каскад (SelectCascade) И чтобы выбрать
// соответствующую lightViewProj[cascadeIndex] для проекции worldPos
// в пространство ИМЕННО этого каскада.
float ComputeShadowFactor(float3 worldPos, float3 normal, float viewDepth) {
    if (shadowsEnabled == 0) {
        return 1.0;
    }
    int cascadeIndex = SelectCascade(viewDepth);
    float4 lightSpacePos = mul(lightViewProj[cascadeIndex], float4(worldPos, 1.0));
    if (lightSpacePos.w <= 0.0001) {
        return 1.0;
    }
    float3 ndc = lightSpacePos.xyz / lightSpacePos.w;
    // NDC X/Y в [-1,1] (DirectX-конвенция) -> UV [0,1] с переворотом
    // Y (текстурные координаты растут ВНИЗ, NDC Y растёт ВВЕРХ) —
    // тот же переворот, что неявно делает растеризатор для
    // обычного экрана, но здесь нужен вручную, т.к. мы читаем
    // shadow map как обычную текстуру, а не через SV_POSITION.
    float2 shadowUV = float2(ndc.x * 0.5 + 0.5, 1.0 - (ndc.y * 0.5 + 0.5));
    float shadowDepth = ndc.z;
    if (shadowUV.x < 0.0 || shadowUV.x > 1.0 || shadowUV.y < 0.0 || shadowUV.y > 1.0 || shadowDepth < 0.0 || shadowDepth > 1.0) {
        return 1.0;
    }
    // Нормаль-based bias: пологие поверхности (свет скользит почти
    // параллельно нормали) страдают от acne сильнее, чем
    // перпендикулярные — компенсируем sqrt(1-NdotL^2) (~tan угла
    // падения), плюс базовый shadowBias для перпендикулярного
    // случая. Оба фактора работают ВМЕСТЕ с аппаратным
    // DepthBias/SlopeScaledDepthBias из shadow PSO (см.
    // create_shadow_pipeline_state) — не дублируют, а
    // подстраховывают друг друга на разных углах.
    float3 toLight = normalize(-lightDir.xyz);
    float ndotl = saturate(dot(normal, toLight));
    float slopeBias = shadowBias * sqrt(saturate(1.0 - ndotl * ndotl)) * 4.0 + shadowBias;
    float biasedDepth = saturate(shadowDepth - slopeBias);
    return SampleShadowPCF(cascadeIndex, float3(shadowUV, biasedDepth));
}

// ДОБАВЛЕНО (тени фонарей): видимость фонаря из точки worldPos
// (1 = освещена, 0 = в тени) по ОДНОЙ перспективной плитке атласа
// (spot — плитка фонаря, point — грань куба), 3x3 PCF как у солнца.
//
// Bias — normal offset: точка сдвигается вдоль нормали на размер
// ОДНОГО текселя плитки на этой дистанции от фонаря (у перспективы
// тексель растёт линейно с расстоянием: 2*d*tan(fov/2)/res), больше
// на скользящих углах. Это убирает acne без peter-panning, которым
// страдает постоянный сдвиг глубины у перспективных карт.
//
// PCF-выборки зажаты внутрь своей плитки (минус 1.5 текселя с
// краёв), иначе фильтр на краю читал бы глубину СОСЕДНЕЙ плитки.
float SampleShadowTile(Texture2D atlas, float4x4 viewProj, uint tile, float4 atlasParams, float tanHalfFov,
                       float3 worldPos, float3 normal, float3 toL, float dist) {
    float tileRes = max(atlasParams.z, 1.0);
    float texelWorld = 2.0 * dist * tanHalfFov / tileRes;
    float ndotl = saturate(dot(normal, toL));
    float3 offsetPos = worldPos + normal * texelWorld * (1.0 + 2.0 * (1.0 - ndotl));

    float4 clip = mul(viewProj, float4(offsetPos, 1.0));
    if (clip.w <= 0.0001) {
        return 1.0;
    }
    float3 ndc = clip.xyz / clip.w;
    if (abs(ndc.x) > 1.0 || abs(ndc.y) > 1.0 || ndc.z < 0.0 || ndc.z > 1.0) {
        return 1.0;
    }
    float2 localUV = float2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    uint perRow = (uint)atlasParams.x;
    float2 tileOrigin = float2(tile % perRow, tile / perRow);
    float atlasTexel = atlasParams.y / tileRes;
    float2 tileMin = tileOrigin * atlasParams.y + atlasTexel * 1.5;
    float2 tileMax = (tileOrigin + 1.0) * atlasParams.y - atlasTexel * 1.5;
    float2 atlasUV = (tileOrigin + localUV) * atlasParams.y;

    float sum = 0.0;
    [unroll]
    for (int y = -1; y <= 1; y++) {
        [unroll]
        for (int x = -1; x <= 1; x++) {
            float2 uv = clamp(atlasUV + float2(x, y) * atlasTexel, tileMin, tileMax);
            sum += atlas.SampleCmpLevelZero(ShadowSampler, uv, ndc.z);
        }
    }
    return sum / 9.0;
}

float SampleSpotShadow(GPULight light, float3 worldPos, float3 normal, float3 toL, float dist) {
    uint tile = (uint)(light.params.w + 0.5) - 1;
    return SampleShadowTile(SpotShadowAtlas, spotShadowViewProj[tile], tile, spotShadowParams,
                            tan(light.params.x + 0.05), worldPos, normal, toL, dist);
}

// ДОБАВЛЕНО (тени point-фонарей): грань куба выбирается по главной
// оси вектора ОТ фонаря К точке — порядок граней +X,-X,+Y,-Y,+Z,-Z
// ОБЯЗАН совпадать с compute_point_shadow_view_projs (Rust). Грани
// отрисованы с углом чуть больше 90° (pointShadowParams.w), поэтому
// точка на стыке граней лежит внутри плитки с запасом под PCF.
float SamplePointShadow(GPULight light, float3 worldPos, float3 normal, float3 toL, float dist) {
    uint lightSlot = (uint)(-light.params.w + 0.5) - 1;
    float3 v = worldPos - light.position.xyz;
    float3 a = abs(v);
    uint face;
    if (a.x >= a.y && a.x >= a.z) {
        face = v.x > 0.0 ? 0 : 1;
    } else if (a.y >= a.z) {
        face = v.y > 0.0 ? 2 : 3;
    } else {
        face = v.z > 0.0 ? 4 : 5;
    }
    uint tile = lightSlot * 6 + face;
    return SampleShadowTile(PointShadowAtlas, pointShadowViewProj[tile], tile, pointShadowParams,
                            pointShadowParams.w, worldPos, normal, toL, dist);
}

// ОБНОВЛЕНО (Фаза 4 плана по реализму/фонарям): раньше Point (0) и
// Spot (1) обрабатывались АБСОЛЮТНО одинаково — условие `lightType
// > 1.5` отделяло только Directional (2) от всех остальных, то
// есть уличный фонарь, помеченный как Spot, светил равномерно во
// все стороны как голая лампочка, что и есть главная претензия
// пользователя ("фонари надо рисовать как в реальности"). Теперь
// Spot получает настоящий конус: params.x = spot_outer_angle,
// params.z = spot_inner_angle (радианы, угол от оси direction).
// Между inner и outer — плавный smoothstep-переход ("мягкое
// пятно", похожее по ощущению на реальный отражатель фонаря, без
// полноценного IES-профиля, который остаётся отдельным будущим
// улучшением для топового железа, а не обязательным минимумом).
// Вынесено в отдельную функцию, чтобы grid-путь (основной) и
// fallback-путь (полный перебор, см. ниже) считали освещение
// ИДЕНТИЧНО, не двумя разными формулами, которые могли бы
// незаметно разойтись при будущих правках.
float3 ComputePointLightContribution(GPULight light, float3 worldPos, float3 normal, out float3 toL) {
    float lightType = light.position.w; // 0=Point,1=Spot,2=Directional
    float attenuation;
    if (lightType > 1.5) {
        // Directional-фонарь из списка FirstFires (редкий случай —
        // обычно directional задаётся отдельно через
        // lightDir/lightColor выше в main(), но поддерживаем и
        // здесь для полноты, если такой свет добавят через .alfar).
        toL = normalize(-light.direction.xyz);
        attenuation = 1.0;
    } else {
        // Point и Spot: общее для обоих — честное inverse-square
        // затухание по расстоянию с плавным обнулением к границе
        // range (window function — без неё виден резкий обрыв
        // освещённости ровно на границе радиуса действия фонаря).
        float3 toLightVec = light.position.xyz - worldPos;
        float dist = length(toLightVec);
        toL = dist > 0.0001 ? (toLightVec / dist) : float3(0.0, 1.0, 0.0);
        float range = max(light.direction.w, 0.001);
        float distRatio = saturate(dist / range);
        float windowFalloff = (1.0 - distRatio * distRatio);
        windowFalloff = windowFalloff * windowFalloff;
        float invSquare = 1.0 / max(dist * dist, 0.01);
        attenuation = invSquare * windowFalloff;

        if (lightType > 0.5) {
            // Spot: дополнительный конусный множитель. direction.xyz
            // — направление, КУДА светит фонарь (не "к фонарю", а
            // "от фонаря") — поэтому сравниваем с -toL (вектор ОТ
            // фонаря К пикселю), а не с toL (вектор К фонарю).
            float3 spotDir = normalize(light.direction.xyz);
            float cosAngle = dot(spotDir, -toL);
            float cosOuter = cos(max(light.params.x, 0.001));
            float cosInner = cos(max(min(light.params.z, light.params.x), 0.0));
            // saturate: если inner>=outer (некорректные/нулевые
            // данные — например params.z не задан для старой сцены,
            // добавленной через add_street_light без spot-полей),
            // smoothstep(cosOuter, cosInner, x) с cosInner<=cosOuter
            // даёт корректный резкий, но не мусорный переход, а не
            // NaN/деление на 0.
            float coneFactor = smoothstep(cosOuter, max(cosInner, cosOuter + 0.0001), cosAngle);
            attenuation *= coneFactor;
            // ДОБАВЛЕНО (тени фонарей): фонарь из числа
            // тенеобразующих в этом кадре (params.w > 0) — свет не
            // проходит сквозь машины/стены/столбы. Выборку делаем
            // только если точка вообще освещена конусом.
            if (light.params.w > 0.5 && attenuation > 0.0) {
                attenuation *= SampleSpotShadow(light, worldPos, normal, toL, dist);
            }
        } else if (light.params.w < -0.5 && attenuation > 0.0) {
            // ДОБАВЛЕНО (тени point-фонарей): cube-тень, 6 граней.
            attenuation *= SamplePointShadow(light, worldPos, normal, toL, dist);
        }
    }
    // ИЗМЕНЕНО (честный PBR для фонарей): возвращаем ВХОДЯЩУЮ
    // энергию на единицу NdotL (цвет * intensity * затухание/конус)
    // и направление на свет (out toL) — BRDF (диффуз + GGX-специуляр)
    // применяется снаружи, в ShadeLight, ОДИНАКОВО для солнца и
    // фонарей. Раньше фонари давали только Ламберт без блика —
    // мокрый асфальт/металл под фонарём выглядел матовым.
    float intensity = light.color.w;
    return light.color.rgb * intensity * attenuation;
}

// ДОБАВЛЕНО (светящиеся плафоны — "фонари светят, а сами плафоны тёмные,
// как в темноте"): точка света фонаря стоит в центре его рассеивателя,
// но сама по себе невидима — а рассеиватель освещался только отражённым
// светом, хотя на деле это и ЕСТЬ источник. Яркость излучающей
// поверхности берём из ТОГО ЖЕ фонаря, а не из материала: плоский матовый
// рассеиватель площади A, дающий по оси силу света I, имеет яркость
// L = I / A (ламбертовский излучатель). intensity здесь — та же величина,
// что даёт color·intensity/d² в ComputePointLightContribution, так что
// плафон и пятно света под ним согласованы физически, а не подобраны на
// глаз. Плафон гаснет днём, мигает и окрашен ровно так же, как его свет.
//
// Связь "пиксель плафона ↔ его фонарь": фонарь ближе EMITTER_LINK_RADIUS
// к пикселю (рассеиватель ~0.5 м, соседние фонари — минимум в 10 м), а
// для spot — поверхность смотрит туда, куда светит фонарь (свет выходит
// через эту грань, а не через крышку корпуса).
static const float EMITTER_LINK_RADIUS = 0.6;

float3 EmitterRadiance(GPULight light, float3 worldPos, float3 surfaceNormal) {
    if (light.position.w > 1.5) {
        return float3(0.0, 0.0, 0.0); // directional — не точечный излучатель
    }
    float3 d = worldPos - light.position.xyz;
    if (dot(d, d) > EMITTER_LINK_RADIUS * EMITTER_LINK_RADIUS) {
        return float3(0.0, 0.0, 0.0);
    }
    if (light.position.w > 0.5 && dot(surfaceNormal, normalize(light.direction.xyz)) <= 0.0) {
        return float3(0.0, 0.0, 0.0);
    }
    return light.color.rgb * max(light.color.w, 0.0) / max(lightEmitterArea, 0.0001);
}

// ДОБАВЛЕНО (Задача #15, normal mapping — PBR-специуляр):
// Cook-Torrance микрофасетная модель с GGX/Trowbridge-Reitz
// распределением нормалей (D), Smith-геометрией с
// Schlick-GGX-аппроксимацией (G) и Schlick-аппроксимацией Френеля
// (F) — стандартная тройка функций physically-based specular,
// применяется ко ВСЕМ источникам — солнцу и point/spot-фонарям
// (см. ShadeLight ниже; раньше фонари были на Ламберте).
//
// Диэлектрики (metallic=0) используют F0=0.04 (стандартное
// приближение для большинства неметаллов — стекло, пластик,
// камень), металлы (metallic=1) используют сам albedo как F0
// (металлы отражают тем же цветом, каким выглядит их диффуз) —
// линейная интерполяция между ними по metallic, тот же приём, что
// в стандартном glTF/Disney PBR.
float DistributionGGX(float3 N, float3 H, float roughness) {
    float a = roughness * roughness;
    float a2 = a * a;
    float NdotH = max(dot(N, H), 0.0);
    float NdotH2 = NdotH * NdotH;
    float denom = (NdotH2 * (a2 - 1.0) + 1.0);
    denom = 3.14159265 * denom * denom;
    return a2 / max(denom, 0.0001);
}
float GeometrySchlickGGX(float NdotV, float roughness) {
    float r = roughness + 1.0;
    float k = (r * r) / 8.0;
    return NdotV / max(NdotV * (1.0 - k) + k, 0.0001);
}
float GeometrySmith(float3 N, float3 V, float3 L, float roughness) {
    float NdotV = max(dot(N, V), 0.0);
    float NdotL = max(dot(N, L), 0.0);
    return GeometrySchlickGGX(NdotV, roughness) * GeometrySchlickGGX(NdotL, roughness);
}
float3 FresnelSchlick(float cosTheta, float3 F0) {
    return F0 + (1.0 - F0) * pow(saturate(1.0 - cosTheta), 5.0);
}
// Возвращает ПОЛНЫЙ вклад источника (диффуз + specular).
// `radiancePerNdotL` — цвет * intensity * затухание * тень, БЕЗ NdotL.
//
// ИЗМЕНЕНО (честный PBR): одна функция полного Cook-Torrance для
// ЛЮБОГО источника (солнце, point, spot) вместо "GGX только для
// солнца, Ламберт для фонарей". Исправлены две физические ошибки
// прежней версии:
//  1) Сохранение энергии: диффуз домножается на kD = (1-F)(1-metallic)
//     — свет, отражённый зеркально (F), не может ещё и рассеяться
//     диффузно. Раньше под скользящим углом поверхность получала
//     И полный диффуз, И сильный Френель — больше энергии, чем пришло.
//  2) Нормировка: сцены отстроены под diffuse = albedo*I*NdotL
//     (без деления на PI), т.е. `I` здесь = E/PI. Корректный
//     specular при той же конвенции = BRDF * PI * I * NdotL.
//     Раньше множителя PI не было — блики были в ~3 раза слабее
//     физически правильных относительно диффуза. Диффуз НЕ менялся,
//     поэтому яркость существующих сцен (.alfar) остаётся прежней.
float3 ShadeLight(float3 N, float3 V, float3 L, float3 albedo, float metallic, float roughness, float3 radiancePerNdotL) {
    float NdotL = max(dot(N, L), 0.0);
    if (NdotL <= 0.0) {
        return float3(0.0, 0.0, 0.0);
    }
    float3 H = normalize(V + L);
    float3 F0 = lerp(float3(0.04, 0.04, 0.04), albedo, metallic);
    float NDF = DistributionGGX(N, H, roughness);
    float G = GeometrySmith(N, V, L, roughness);
    float3 F = FresnelSchlick(max(dot(H, V), 0.0), F0);
    float3 specular = (NDF * G * F) / (4.0 * max(dot(N, V), 0.0) * NdotL + 0.0001);
    float3 kD = (1.0 - F) * (1.0 - metallic);
    return (kD * albedo + specular * 3.14159265) * radiancePerNdotL * NdotL;
}

// ДОБАВЛЕНО (честный PBR — ambient): ambientColor — равномерное
// рассеянное освещение со всех сторон. Для такого окружения
// интеграл GGX-специуляра по полусфере не ноль: металлы и гладкие
// поверхности ОТРАЖАЮТ окружение, а не рассеивают его. Раньше
// ambient давал только диффуз albedo*A — металл в тени выглядел
// тёмным цветным пластиком. Аналитическая аппроксимация интеграла
// env-BRDF (Karis, "Physically Based Shading on Mobile", 2014) —
// то же, что split-sum LUT, только без текстуры. Для однородного
// окружения это не подделка, а решение с точностью аппроксимации.
float3 EnvBRDFApprox(float3 F0, float roughness, float NdotV) {
    const float4 c0 = float4(-1.0, -0.0275, -0.572, 0.022);
    const float4 c1 = float4(1.0, 0.0425, 1.04, -0.04);
    float4 r = roughness * c0 + c1;
    float a004 = min(r.x * r.x, exp2(-9.28 * NdotV)) * r.x + r.y;
    float2 AB = float2(-1.04, 1.04) * a004 + r.zw;
    return F0 * AB.x + AB.y;
}

// ДОБАВЛЕНО (честный PBR — цветовое пространство): текстуры альбедо
// авторятся в sRGB (так их видит художник на мониторе), а текстура
// создаётся как R8G8B8A8_UNORM без sRGB-вью (asset_loading.rs) —
// GPU отдаёт шейдеру ГАММА-закодированные значения. Освещение в
// гамма-пространстве физически неверно (два фонаря не дают
// удвоения яркости), а tonemap в конце ЕЩЁ РАЗ применяет гамму
// 1/2.2 — текстуры выходили выцветшими. Декодируем точной кривой
// sRGB. Цвет вершин (input.color) по конвенции glTF (COLOR_0,
// baseColorFactor) уже линейный — не трогаем. Normal/MR-карты —
// данные, не цвет — тоже не трогаем.
float3 SRGBToLinear(float3 c) {
    c = saturate(c);
    return c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4);
}

// ДОБАВЛЕНО (честный SSAO): два выхода — полный HDR-цвет и
// отдельно ambient-вклад (см. Renderer::ambient_target). Composite
// затеняет SSAO только ambient-часть.
struct PS_OUTPUT {
    float4 color : SV_Target0;
    float4 ambient : SV_Target1;
};

PS_OUTPUT main(PS_INPUT input) {
    float3 geomNormal = normalize(input.normal);

    // ДОБАВЛЕНО (Задача #15, normal mapping): построение TBN-базиса
    // и трансформация сэмплированной normal map (tangent-space) в
    // world-space. Gram-Schmidt пере-ортогонализация tangent
    // относительно normal — интерполяция по треугольнику (и
    // неравномерный масштаб модели) может немного разбалансировать
    // строгую перпендикулярность, накопленную на этапе экспорта.
    float3 T = normalize(input.tangent.xyz - geomNormal * dot(geomNormal, input.tangent.xyz));
    float3 B = cross(geomNormal, T) * input.tangent.w;
    float3x3 TBN = float3x3(T, B, geomNormal);
    // NormalMap.rgb в [0,1] — декодируем в tangent-space вектор
    // [-1,1]. Fallback-текстура (128,128,255) декодируется РОВНО в
    // (0,0,1) — "нормаль не меняется", см. `ensure_flat_normal_texture`.
    float3 tangentNormal = NormalMap.Sample(MaterialSampler, input.uv).rgb * 2.0 - 1.0;
    float3 normal = normalize(mul(tangentNormal, TBN));

    // lightDir хранится как направление "куда светит" (см.
    // TransformConstants::new(): [0,-1,0,0]) — свет приходит с
    // противоположной стороны, поэтому -lightDir.xyz.
    float3 toLight = normalize(-lightDir.xyz);
    // ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): тени
    // применяются ТОЛЬКО к directional-свету (солнце/луна) — это
    // единственный источник, для которого в этой фазе строится
    // shadow map (см. compute_cascade_view_proj). Point/spot-фонари
    // FirstFires теней пока не отбрасывают — сознательно
    // отложенное расширение (потребовало бы отдельных
    // point/spot shadow map на каждый тенеобразующий фонарь, что
    // на порядок дороже одного directional-прохода) для будущего
    // шага этой же Фазы 6, а не часть минимального рабочего
    // варианта. Ambient-составляющая НЕ затеняется — она по
    // определению не направленная (рассеянный свет неба),
    // затенять её было бы физически неверно (тень стала бы чёрной
    // дырой вместо мягкого рассеянного полумрака).
    //
    // ДОБАВЛЕНО (Cascaded Shadow Maps): view-space Z пикселя — то
    // же соглашение, что и cascade_far_distances в render_frame
    // (engine/mod.rs): расстояние вдоль оси взгляда камеры, не
    // euclidean-дистанция. `view` — матрица камеры (не света),
    // уже доступна в TransformConstants.
    // ИСПРАВЛЕНО (знак): камера правосторонняя — перед ней view z
    // ОТРИЦАТЕЛЕН. Раньше бралось .z без минуса: значение всегда <0,
    // SelectCascade всегда возвращал каскад 0, и всё дальше его
    // границы (8% дальности камеры) оставалось без тени вообще.
    float pixelViewDepth = -mul(view, float4(input.worldPos, 1.0)).z;
    float shadowFactor = ComputeShadowFactor(input.worldPos, normal, pixelViewDepth);

    // ДОБАВЛЕНО (Задача #15, normal mapping): metallic/roughness
    // ЭТОГО меша — из MetallicRoughnessMap (R=metallic, G=roughness),
    // если у меша есть собственная карта (hasMrMap>0.5, см.
    // MaterialConstants), иначе из root constants напрямую.
    float metallic = rootMetallic;
    float roughness = rootRoughness;
    if (hasMrMap > 0.5) {
        float2 mr = MetallicRoughnessMap.Sample(MaterialSampler, input.uv).rg;
        metallic = mr.r;
        roughness = mr.g;
    }
    roughness = clamp(roughness, 0.045, 1.0); // 0 даёт NDF-деление на ~0 (зеркальная точка) — числовая защита

    // ИЗМЕНЕНО (честный PBR): текстура декодируется из sRGB в
    // линейное пространство (см. SRGBToLinear), цвет вершины —
    // часть альбедо (базовый цвет материала), и ТОЛЬКО здесь.
    float3 albedoRaw = input.color.rgb * SRGBToLinear(AlbedoMap.Sample(MaterialSampler, input.uv).rgb);
    float3 viewDir = normalize(cameraPos.xyz - input.worldPos);
    float NdotV = saturate(dot(normal, viewDir));

    // Directional-свет (солнце/луна) — тот же ShadeLight, что и у
    // фонарей ниже.
    float3 sunRadiancePerNdotL = lightColor.rgb * lightColor.a * shadowFactor;
    float3 brightness = ShadeLight(normal, viewDir, toLight, albedoRaw, metallic, roughness, sunRadiancePerNdotL);

    // Ambient: диффуз только у диэлектриков + отражение окружения
    // (EnvBRDFApprox), с тем же kD-балансом энергии, что и у
    // прямого света.
    float3 ambientRadiance = ambientColor.rgb * ambientColor.a;
    float3 F0 = lerp(float3(0.04, 0.04, 0.04), albedoRaw, metallic);
    float3 envSpec = EnvBRDFApprox(F0, roughness, NdotV);
    float3 ambientTerm = ambientRadiance * (albedoRaw * (1.0 - envSpec) * (1.0 - metallic) + envSpec);
    brightness += ambientTerm;

    // ДОБАВЛЕНО (светящиеся плафоны): собственное излучение поверхности —
    // статичное из материала (.altex Material::emissive) плюс, для
    // рассеивателей фонарей (lightEmitterArea > 0), яркость из фонаря,
    // стоящего в них (EmitterRadiance, считается в циклах по фонарям ниже).
    // Геометрическая нормаль, а не из normal map: "в какую сторону смотрит
    // грань" — свойство геометрии, а не микрорельефа.
    //
    // Статичное излучение умножается на albedo (текстура * цвет вершины):
    // картинка материала служит и маской свечения. Так у вывески светятся
    // только буквы атласа, а чёрный фон их ячеек — нет (без этого каждая
    // буква была бы светящимся прямоугольником). Для однотонного материала
    // это просто emissive * его цвет.
    float3 emitted = materialEmissive * albedoRaw;
    bool isLightEmitter = lightEmitterArea > 0.0;

    // ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): находим ячейку
    // сетки, которой принадлежит этот пиксель, и проверяем ТОЛЬКО
    // фонари этой ячейки — вместо перебора всего видимого списка
    // (что делала Фаза 2). Если пиксель вне границ сетки
    // (gridDimensions == 0, т.е. свет ещё не инициализирован, или
    // worldPos буквально за пределами world_min/world_max) —
    // тихий fallback на 0 дополнительных фонарей, directional-свет
    // выше по-прежнему работает.
    if (gridDimensions.x > 0 && gridDimensions.y > 0 && gridDimensions.z > 0) {
        float cellSize = max(gridWorldMin.w, 0.001);
        float3 localPos = input.worldPos - gridWorldMin.xyz;
        int3 cell = int3(floor(localPos / cellSize));

        if (cell.x >= 0 && cell.y >= 0 && cell.z >= 0 &&
            (uint)cell.x < gridDimensions.x && (uint)cell.y < gridDimensions.y && (uint)cell.z < gridDimensions.z) {
            uint cellIndex = (uint)cell.z * gridDimensions.y * gridDimensions.x +
                              (uint)cell.y * gridDimensions.x +
                              (uint)cell.x;
            LightGridCell gridCell = GridCells[cellIndex];

            for (uint e = 0; e < gridCell.count; e++) {
                LightGridEntry entry = GridEntries[gridCell.offset + e];
                GPULight light = Lights[entry.lightIndex];
                // ИЗМЕНЕНО (честный PBR): фонарь проходит через тот
                // же Cook-Torrance (ShadeLight), что и солнце —
                // с бликом и балансом энергии, а не голый Ламберт.
                float3 toL;
                float3 lightRadiance = ComputePointLightContribution(light, input.worldPos, normal, toL);
                brightness += ShadeLight(normal, viewDir, toL, albedoRaw, metallic, roughness, lightRadiance);
                if (isLightEmitter) {
                    emitted += EmitterRadiance(light, input.worldPos, geomNormal);
                }
            }
        }
        // Пиксель вне границ сетки (например очень далёкий объект
        // за world_max) — намеренно 0 фонарных вкладов, а не
        // fallback на полный перебор: за пределами сетки FirstFires
        // всё равно ничего не закуллено в эти координаты.
    } else {
        // Сетка ещё не инициализирована (init_lights() не
        // вызывался, либо LightConfig ещё не применился) — честный
        // fallback: перебор lightCount видимых фонарей напрямую из
        // Lights[], без сетки. Не должен срабатывать в обычном
        // режиме работы движка, но не даёт кадру остаться совсем
        // без фонарей, если сетка почему-то недоступна.
        for (uint i = 0; i < lightCount; i++) {
            float3 toL;
            float3 lightRadiance = ComputePointLightContribution(Lights[i], input.worldPos, normal, toL);
            brightness += ShadeLight(normal, viewDir, toL, albedoRaw, metallic, roughness, lightRadiance);
            if (isLightEmitter) {
                emitted += EmitterRadiance(Lights[i], input.worldPos, geomNormal);
            }
        }
    }

    // ИСПРАВЛЕНО (честный PBR): раньше здесь стояло
    // `input.color.rgb * brightness` — цвет вершины уже входит в
    // albedoRaw выше, так что диффуз получал его В КВАДРАТЕ
    // (серый 0.5 рендерился как 0.25), а блик — окрашивался в цвет
    // диэлектрика, чего в реальности не бывает (блик пластика белый).
    PS_OUTPUT output;
    output.color = float4(brightness + emitted, input.color.a);
    output.ambient = float4(ambientTerm, 0.0);
    return output;
}
