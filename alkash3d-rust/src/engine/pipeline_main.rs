//! Основной 3D draw pass: HLSL-шейдеры (вершинный трансформ + пиксельный —
//! сетка каллинга фонарей, cascaded shadow maps, normal mapping, Cook-Torrance
//! PBR-специуляр), корневая сигнатура и pipeline state объекта прохода.
//!
//! ВЫНЕСЕНО из `engine/mod.rs` (Фаза 1 архитектурного рефакторинга — разбивка
//! монолита `impl AlkashEngine` на подсистемы). Перенос дословный, тела
//! методов не менялись. В оригинальном файле `create_root_signature`/
//! `create_pipeline_state` физически находились ПОСЛЕ всех
//! tonemap/bloom/shadow/occluder/volumetric методов (см. остальные
//! `engine/pipeline_*.rs`) — здесь они объединены с `compile_default_shaders`
//! логически, т.к. все три относятся к одному и тому же основному проходу.

use windows::core::*;
use windows::Win32::Foundation::*;
use crate::STATE;
use crate::shader::ShaderBlob;
use crate::pso::PipelineState;
use super::{AlkashEngine, Vertex, NUM_CASCADES};

impl AlkashEngine {
    pub(super) fn compile_default_shaders(&mut self) -> Result<()> {
        let vs_source = r#"
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
        };

        // ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы): поле `uv` в
        // VS_INPUT/VS_OUTPUT — зеркалит новое поле `uv: [f32;2]` в
        // `engine::Vertex` (TEXCOORD0 элемент input layout, см. pso.rs).
        // TEXCOORD2 у VS_OUTPUT.uv (не TEXCOORD0!) — TEXCOORD0/1 у
        // VS_OUTPUT уже заняты worldPos/normal, а входной семантический
        // индекс input layout (TEXCOORD0 у VS_INPUT.uv) — ОТДЕЛЬНОЕ
        // пространство имён от выходных семантик VS_OUTPUT, переиспользовать
        // индексы между входом/выходом вершинного шейдера можно без
        // конфликта, но здесь сознательно выбран следующий свободный
        // (TEXCOORD2), чтобы не запутывать чтение кода.
        // ДОБАВЛЕНО (Задача #15, normal mapping): TANGENT — зеркалит новое
        // поле `tangent: [f32;4]` в `engine::Vertex` (xyz + w=handedness,
        // см. подробный комментарий там). VS_OUTPUT.tangent — TEXCOORD3
        // (следующий свободный после worldPos@0/normal@1/uv@2).
        struct VS_INPUT {
            float4 pos : POSITION;
            float3 normal : NORMAL;
            float4 color : COLOR;
            float2 uv : TEXCOORD0;
            float4 tangent : TANGENT;
        };
        struct VS_OUTPUT {
            float4 pos : SV_POSITION;
            float4 color : COLOR;
            float3 worldPos : TEXCOORD0;
            float3 normal : TEXCOORD1;
            float2 uv : TEXCOORD2;
            float4 tangent : TEXCOORD3;
        };
        VS_OUTPUT main(VS_INPUT input) {
            VS_OUTPUT output;
            output.pos = mul(modelViewProj, input.pos);
            output.color = input.color;
            output.worldPos = mul(model, input.pos).xyz;
            // Верхний 3x3 блок model — вращение/масштаб, без переноса;
            // этого достаточно для направлений при равномерном scale.
            float3x3 normalMatrix = (float3x3)model;
            output.normal = normalize(mul(normalMatrix, input.normal));
            output.uv = input.uv;
            // ДОБАВЛЕНО (Задача #15, normal mapping): tangent преобразуется
            // ТОЙ ЖЕ normalMatrix, что и normal (оба — направления, не
            // точки, w-компонента handedness переносится как есть — знак
            // не зависит от вращения/равномерного масштаба).
            output.tangent = float4(normalize(mul(normalMatrix, input.tangent.xyz)), input.tangent.w);
            return output;
        }
        "#;

        let ps_source = r#"
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
        cbuffer MaterialConstants : register(b1) {
            float rootMetallic;
            float rootRoughness;
            float hasMrMap;
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
        float3 ComputePointLightContribution(GPULight light, float3 worldPos, float3 normal) {
            float lightType = light.position.w; // 0=Point,1=Spot,2=Directional
            float3 toL;
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
                }
            }
            float lightDiff = max(dot(normal, toL), 0.0);
            float intensity = light.color.w;
            return light.color.rgb * intensity * lightDiff * attenuation;
        }

        // ДОБАВЛЕНО (Задача #15, normal mapping — PBR-специуляр):
        // Cook-Torrance микрофасетная модель с GGX/Trowbridge-Reitz
        // распределением нормалей (D), Smith-геометрией с
        // Schlick-GGX-аппроксимацией (G) и Schlick-аппроксимацией Френеля
        // (F) — стандартная тройка функций physically-based specular,
        // применяется здесь ТОЛЬКО к directional-свету (солнце/луна,
        // единственный источник, отбрасывающий тени и визуально
        // доминирующий в кадре) — это сознательно консервативный первый
        // шаг PBR-specular: point/spot-фонари FirstFires остаются на
        // прежней Lambertian-модели (ComputePointLightContribution выше),
        // не переписанной в этом шаге, чтобы не рисковать регрессией уже
        // работающего городского освещения ради специуляра, который на
        // маленьких point-источниках визуально менее заметен, чем на ярком
        // направленном свете.
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
        // Возвращает СПЕЦИУЛЯРНУЮ (не диффузную — та считается снаружи, как
        // и раньше) добавку Cook-Torrance для directional-света. `radiance`
        // — уже посчитанный вклад источника (цвет * intensity * NdotL *
        // shadowFactor) — та же величина, что раньше целиком уходила в
        // diffuse; здесь распределяется между диффузом (снаружи, домножен
        // на (1-metallic) — металлы не имеют диффузного отклика) и этим
        // specular-членом.
        float3 ComputeSpecularGGX(float3 N, float3 V, float3 L, float3 albedo, float metallic, float roughness, float3 radiancePerNdotL) {
            float3 H = normalize(V + L);
            float NdotL = max(dot(N, L), 0.0);
            float3 F0 = lerp(float3(0.04, 0.04, 0.04), albedo, metallic);
            float NDF = DistributionGGX(N, H, roughness);
            float G = GeometrySmith(N, V, L, roughness);
            float3 F = FresnelSchlick(max(dot(H, V), 0.0), F0);
            float3 numerator = NDF * G * F;
            float denom = 4.0 * max(dot(N, V), 0.0) * NdotL + 0.0001;
            float3 specular = numerator / denom;
            return specular * radiancePerNdotL * NdotL;
        }

        float4 main(PS_INPUT input) : SV_TARGET {
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
            float diff = max(dot(normal, toLight), 0.0);
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
            float pixelViewDepth = mul(view, float4(input.worldPos, 1.0)).z;
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

            float3 albedoRaw = input.color.rgb * AlbedoMap.Sample(MaterialSampler, input.uv).rgb;
            float3 viewDir = normalize(cameraPos.xyz - input.worldPos);

            // Directional-свет: диффуз (домножен на (1-metallic) —
            // металлы физически не имеют диффузного отклика, вся энергия
            // уходит в specular) + Cook-Torrance GGX-специуляр (см. функции
            // выше). radiancePerNdotL — тот же множитель, что раньше шёл
            // целиком в diffuse, БЕЗ повторного домножения на diff здесь
            // (ComputeSpecularGGX сам домножает на NdotL внутри).
            float3 sunRadiancePerNdotL = lightColor.rgb * lightColor.a * shadowFactor;
            float3 diffuse = albedoRaw * (1.0 - metallic) * sunRadiancePerNdotL * diff;
            float3 specular = ComputeSpecularGGX(normal, viewDir, toLight, albedoRaw, metallic, roughness, sunRadiancePerNdotL);
            float3 ambient = ambientColor.rgb * ambientColor.a * albedoRaw;
            float3 brightness = ambient + diffuse + specular;

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
                        // ИЗМЕНЕНО (Задача #15, normal mapping): вклад
                        // point/spot-фонаря теперь домножается на albedoRaw
                        // ЗДЕСЬ, а не единым умножением в конце функции —
                        // раньше albedo применялось ко ВСЕМУ brightness
                        // разом (ambient+directional+point) одним
                        // умножением в самой последней строке функции; та
                        // схема перестала подходить, когда добавился
                        // GGX-специуляр (specular НЕ должен домножаться на
                        // albedo для диэлектриков — Cook-Torrance уже сам
                        // корректно взвешивает albedo через F0 только для
                        // металлов, см. ComputeSpecularGGX). Результат для
                        // point-фонарей бит-в-бит идентичен старой схеме
                        // (то же самое умножение, просто раньше — в конце).
                        brightness += ComputePointLightContribution(light, input.worldPos, normal) * albedoRaw;
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
                    brightness += ComputePointLightContribution(Lights[i], input.worldPos, normal) * albedoRaw;
                }
            }

            // ИЗМЕНЕНО (Задача #15, normal mapping): albedo уже применено
            // выше — к ambient/diffuse явно (см. `ambient`/`diffuse` строки
            // выше, обе домножены на `albedoRaw`) и к каждому point/spot
            // вкладу в циклах выше. `specular` НЕ домножается на albedo
            // (физически корректно — Cook-Torrance сам взвешивает через
            // F0=lerp(0.04,albedo,metallic), см. ComputeSpecularGGX).
            // Вершинный цвет (`input.color`) остаётся отдельным независимым
            // множителем поверх ВСЕГО результата, как и было исторически —
            // единственное отличие от версии до normal mapping: albedo
            // раньше умножался на brightness ЦЕЛИКОМ одной строкой здесь, а
            // теперь распределён по компонентам выше (для diffuse-
            // диэлектриков результат идентичен; отличие только в появлении
            // specular и metallic-взвешивания, которых раньше не было).
            return float4(input.color.rgb * brightness, input.color.a);
        }
        "#;

        self.vs = Some(ShaderBlob::compile(vs_source, "vs_5_0", "main")?);
        self.ps = Some(ShaderBlob::compile(ps_source, "ps_5_0", "main")?);

        println!("[ENGINE] ✓ Default shaders compiled (нормали + сетка каллинга + spot-конус фонарей)");
        Ok(())
    }

    pub(super) fn create_root_signature(&mut self) -> Result<()> {
        use windows::Win32::Graphics::Direct3D12::*;

        let shadow_srv_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: NUM_CASCADES as u32,
            BaseShaderRegister: 3,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        };

        let material_srv_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 1,
            BaseShaderRegister: 6,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        };

        let normal_srv_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 1,
            BaseShaderRegister: 7,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        };
        let mr_srv_range = D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 1,
            BaseShaderRegister: 8,
            RegisterSpace: 0,
            OffsetInDescriptorsFromTableStart: 0,
        };

        let shadow_sampler = D3D12_STATIC_SAMPLER_DESC {
            Filter: D3D12_FILTER_COMPARISON_MIN_MAG_LINEAR_MIP_POINT,
            AddressU: D3D12_TEXTURE_ADDRESS_MODE_BORDER,
            AddressV: D3D12_TEXTURE_ADDRESS_MODE_BORDER,
            AddressW: D3D12_TEXTURE_ADDRESS_MODE_BORDER,
            MipLODBias: 0.0,
            MaxAnisotropy: 0,
            ComparisonFunc: D3D12_COMPARISON_FUNC_LESS,
            BorderColor: D3D12_STATIC_BORDER_COLOR_OPAQUE_WHITE,
            MinLOD: 0.0,
            MaxLOD: D3D12_FLOAT32_MAX,
            ShaderRegister: 0,
            RegisterSpace: 0,
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        };

        // ИЗМЕНЕНО (максимальная графика — анизотропная фильтрация):
        // раньше был обычный трилинейный фильтр — текстуры земли/дороги,
        // видимые под острым углом (главный случай, где это вообще
        // заметно — плоскости пола/двора, уходящие к горизонту), мылились
        // сильнее, чем нужно. 16x — максимум, который гарантированно
        // поддерживает любое D3D12-совместимое железо (FEATURE_LEVEL_11_0+
        // требует минимум 16x у ANISOTROPIC), дороже линейной фильтрации
        // не по числу текселей на пиксель, а по числу samples ПРИ
        // сэмплировании под углом — на прямой взгляд сверху вниз (как у
        // MinLOD/MaxLOD выше) практически бесплатно.
        let material_sampler = D3D12_STATIC_SAMPLER_DESC {
            Filter: D3D12_FILTER_ANISOTROPIC,
            AddressU: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
            AddressV: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
            AddressW: D3D12_TEXTURE_ADDRESS_MODE_WRAP,
            MipLODBias: 0.0,
            MaxAnisotropy: 16,
            ComparisonFunc: D3D12_COMPARISON_FUNC_NEVER,
            BorderColor: D3D12_STATIC_BORDER_COLOR_TRANSPARENT_BLACK,
            MinLOD: 0.0,
            MaxLOD: D3D12_FLOAT32_MAX,
            ShaderRegister: 1,
            RegisterSpace: 0,
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        };

        let static_samplers = [shadow_sampler, material_sampler];

        let root_params = [
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_SRV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 0,
                        RegisterSpace: 0,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_SRV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 1,
                        RegisterSpace: 0,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_SRV,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Descriptor: D3D12_ROOT_DESCRIPTOR {
                        ShaderRegister: 2,
                        RegisterSpace: 0,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: 1,
                        pDescriptorRanges: &shadow_srv_range,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: 1,
                        pDescriptorRanges: &material_srv_range,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: 1,
                        pDescriptorRanges: &normal_srv_range,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: 1,
                        pDescriptorRanges: &mr_srv_range,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
            D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    Constants: D3D12_ROOT_CONSTANTS {
                        ShaderRegister: 1,
                        RegisterSpace: 0,
                        Num32BitValues: 4,
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
            },
        ];

        let root_signature_desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: root_params.len() as u32,
            pParameters: root_params.as_ptr(),
            NumStaticSamplers: static_samplers.len() as u32,
            pStaticSamplers: static_samplers.as_ptr(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };

        let device = crate::get_device()?;

        let mut signature_serialized = None;
        let mut error_blob = None;

        unsafe {
            let hr = D3D12SerializeRootSignature(
                &root_signature_desc,
                D3D_ROOT_SIGNATURE_VERSION_1,
                &mut signature_serialized,
                Some(&mut error_blob),
            );

            if hr.is_err() {
                if let Some(err) = error_blob {
                    let err_data = std::slice::from_raw_parts(
                        err.GetBufferPointer() as *const u8,
                        err.GetBufferSize(),
                    );
                    eprintln!("Root signature error: {}", String::from_utf8_lossy(err_data));
                }
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            let blob = signature_serialized.unwrap();
            let blob_data = std::slice::from_raw_parts(
                blob.GetBufferPointer() as *const u8,
                blob.GetBufferSize(),
            );

            let root_sig = device.CreateRootSignature(0, blob_data)?;
            self.root_signature = Some(root_sig);
        }

        println!("[ENGINE] ✓ Root signature created (CBV b0 + SRV t0 фонари + SRV t1/t2 сетка каллинга + SRV table t3..t5 shadow map + SRV table t6 albedo + comparison sampler s0 + linear sampler s1)");
        Ok(())
    }

    pub(super) fn create_pipeline_state(&mut self) -> Result<()> {
        let vs = self.vs.as_ref().unwrap();
        let ps = self.ps.as_ref().unwrap();
        let root_sig = self.root_signature.as_ref().unwrap();

        let pso = PipelineState::create_graphics(
            vs, ps, root_sig,
            Vertex::STRIDE,
            windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16G16B16A16_FLOAT,
            windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_D32_FLOAT,
            super::MSAA_SAMPLES,
        )?;

        self.pipeline_state = Some(pso);
        println!("[ENGINE] ✓ Pipeline state created (RTV format = R16G16B16A16_FLOAT, matches HDR target, {}x MSAA)", super::MSAA_SAMPLES);
        Ok(())
    }
}
