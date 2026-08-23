// src/render.rs
use windows::core::*;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Direct3D::D3D10_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R32_TYPELESS, DXGI_FORMAT_D32_FLOAT, DXGI_FORMAT_R32_FLOAT, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::DXGI_PRESENT;
use crate::STATE;
use crate::buffer::Buffer;

// Простая структура текстуры для рендера
pub struct RenderTexture {
    pub resource: ID3D12Resource,
    pub width: u32,
    pub height: u32,
    pub format: DXGI_FORMAT,
    pub mip_levels: u32,
}

impl RenderTexture {
    /// ОБНОВЛЕНО (Фаза 8 плана по реализму/фонарям — volumetric-подсветка):
    /// раньше создавался напрямую с конкретным форматом DXGI_FORMAT_D32_FLOAT
    /// (только DSV, никогда не читался как текстура — основной 3D-проход
    /// только писал/тестировал по нему глубину). Volumetric raymarch-проходу
    /// (см. `create_volumetric_shaders`/`render_frame` в engine/mod.rs)
    /// нужно ВОССТАНАВЛИВАТЬ мировую позицию каждого экранного пикселя из
    /// сохранённой глубины, чтобы промаршировать луч камера->пиксель через
    /// shadow map и посчитать, сколько световых лучей "солнца" реально
    /// проходит сквозь сцену (god rays/light shafts) — а для этого нужен
    /// SRV поверх depth-таргета, чего конкретный D32_FLOAT формат не
    /// разрешает (тот же самый ограничение D3D12, что уже решалось для
    /// shadow map, см. подробный комментарий у `create_shadow_map` ниже).
    /// Тот же TYPELESS-паттерн: ресурс R32_TYPELESS, DSV с D32_FLOAT (см.
    /// `create_dsv`), SRV с R32_FLOAT (см. `create_depth_srv` — НОВЫЙ метод,
    /// отдельный от `create_shadow_srv`, хотя оба идентичны по содержимому,
    /// т.к. семантически это разные ресурсы с разным временем жизни и было
    /// бы запутывающим переиспользовать один метод под "любой depth SRV").
    pub fn create_depth_stencil(width: u32, height: u32) -> Result<Self> {
        println!("[RENDERER] Creating depth stencil: {}x{}", width, height);

        // ИСПРАВЛЕНО: было `state.device.as_ref().unwrap().clone()`.
        let device = crate::get_device()?;

        let heap_properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
        };

        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_R32_TYPELESS,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
        };

        let clear_value = D3D12_CLEAR_VALUE {
            Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_D32_FLOAT,
            Anonymous: D3D12_CLEAR_VALUE_0 { DepthStencil: D3D12_DEPTH_STENCIL_VALUE { Depth: 1.0, Stencil: 0 } },
        };

        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                D3D12_RESOURCE_STATE_DEPTH_WRITE,
                Some(&clear_value),
                &mut resource,
            )?;

            let resource = resource.ok_or_else(|| {
                eprintln!("[RENDERER] ERROR: Depth stencil resource is None!");
                Error::from_hresult(HRESULT(1))
            })?;
            println!("[RENDERER] ✓ Depth stencil created");

            Ok(Self {
                resource,
                width,
                height,
                // TYPELESS, как и у shadow map — см. комментарий выше и у
                // `create_shadow_map::format`. `create_dsv`/`create_depth_srv`
                // сами подставляют конкретный формат для своего вида.
                format: DXGI_FORMAT_R32_TYPELESS,
                mip_levels: 1,
            })
        }
    }

    /// ДОБАВЛЕНО (Фаза 8 плана по реализму/фонарям — volumetric-подсветка):
    /// SRV-вид основного depth-таргета (для чтения в volumetric raymarch-
    /// шейдере) — по содержимому идентичен `create_shadow_srv`, но
    /// оставлен отдельным методом (см. подробное обоснование у
    /// `create_depth_stencil` выше).
    pub fn create_depth_srv(&self, handle: D3D12_CPU_DESCRIPTOR_HANDLE) -> Result<()> {
        let device = crate::get_device()?;
        let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R32_FLOAT,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MostDetailedMip: 0,
                    MipLevels: 1,
                    PlaneSlice: 0,
                    ResourceMinLODClamp: 0.0,
                },
            },
        };
        unsafe {
            device.CreateShaderResourceView(&self.resource, Some(&desc), handle);
        }
        Ok(())
    }

    /// ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям): HDR render target
    /// (R16G16B16A16_FLOAT, ALLOW_RENDER_TARGET) — см. подробное
    /// объяснение у поля `Renderer::hdr_target`. В отличие от back
    /// buffer'а (создаётся через swap_chain.GetBuffer — тот всегда LDR,
    /// формат навязан DXGI), эта текстура выделяется явно, с плавающей
    /// точкой, специально чтобы НЕ терять яркость выше 1.0 до тонмаппинга.
    pub fn create_hdr_target(width: u32, height: u32) -> Result<Self> {
        println!("[RENDERER] Creating HDR render target: {}x{}", width, height);

        let device = crate::get_device()?;

        let heap_properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
        };

        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
        };

        let clear_value = D3D12_CLEAR_VALUE {
            Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
            Anonymous: D3D12_CLEAR_VALUE_0 { Color: [0.0, 0.0, 0.0, 1.0] },
        };

        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                // ВАЖНО: изначальное состояние — RENDER_TARGET, так как
                // draw pass рисует в неё как в RTV сразу с первого кадра
                // (в отличие от, например, upload-текстур, которым нужен
                // COMMON/GENERIC_READ до первой записи).
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                Some(&clear_value),
                &mut resource,
            )?;

            let resource = resource.ok_or_else(|| {
                eprintln!("[RENDERER] ERROR: HDR target resource is None!");
                Error::from_hresult(HRESULT(1))
            })?;
            println!("[RENDERER] ✓ HDR render target created");

            Ok(Self {
                resource,
                width,
                height,
                format: DXGI_FORMAT_R16G16B16A16_FLOAT,
                mip_levels: 1,
            })
        }
    }

    /// ДОБАВЛЕНО (Фаза 6 плана по реализму/фонарям — тени): shadow map для
    /// directional-света ("солнца"). В отличие от `create_depth_stencil`
    /// выше (используется ТОЛЬКО как DSV для основного прохода — глубина
    /// пишется и читается GPU при тестировании, но никогда не читается
    /// шейдером как текстура), эта текстура должна одновременно: (1)
    /// писаться как depth-target во время прохода "рендер сцены с точки
    /// зрения света", (2) читаться КАК SRV в основном пиксельном шейдере
    /// (сравнение depth текущего пикселя с сохранённой глубиной — "виден
    /// ли этот пиксель источнику света, или его закрывает что-то ближе").
    ///
    /// D3D12 не разрешает создать ресурс сразу с обоими "чистыми" depth
    /// (D32_FLOAT) и SRV (R32_FLOAT) форматами одновременно — стандартное
    /// решение: создать ресурс с TYPELESS-форматом (R32_TYPELESS, самим по
    /// себе не привязанным ни к глубине, ни к цвету), а формат указать
    /// отдельно у каждого ВИДА (view) поверх него — DSV с D32_FLOAT (см.
    /// `create_dsv` ниже) и SRV с R32_FLOAT (см. `create_shadow_srv` ниже).
    /// Это тот же типизированный ресурс, две разные "линзы" на одну и ту
    /// же память.
    ///
    /// Разрешение 2048x2048 — сознательный выбор под зафиксированный
    /// минимум железа (RTX 3050 8GB, i3-12100F): один такой depth-таргет
    /// занимает 2048*2048*4 байта = 16 МБ видеопамяти, что пренебрежимо
    /// мало на фоне 8 ГБ бюджета, но даёт достаточно плотности пикселей
    /// на тень, чтобы избежать грубого "лестничного" края тени (aliasing)
    /// без каскадов — единственный каскад пока подгоняется под весь
    /// видимый frustum камеры (см. `compute_shadow_view_proj` в
    /// engine/mod.rs), что ограничивает резкость теней на большом
    /// расстоянии обзора. Полноценные каскады (CSM, несколько таких
    /// таргетов на разные дистанции) — сознательно отложенное расширение
    /// этой же Фазы 6 под большие открытые пространства города, а не
    /// часть минимального рабочего варианта.
    pub fn create_shadow_map(resolution: u32) -> Result<Self> {
        println!("[RENDERER] Creating shadow map: {}x{}", resolution, resolution);

        let device = crate::get_device()?;

        let heap_properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
        };

        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: resolution as u64,
            Height: resolution,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_R32_TYPELESS,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL,
        };

        // Clear value ОБЯЗАН объявлять конкретный (не typeless) формат —
        // D32_FLOAT, тот же, что будет у DSV-вида ниже. Depth=1.0 —
        // "максимально далеко", стандартное значение очистки для
        // reversed-less-than depth теста (D3D12_COMPARISON_FUNC_LESS).
        let clear_value = D3D12_CLEAR_VALUE {
            Format: DXGI_FORMAT_D32_FLOAT,
            Anonymous: D3D12_CLEAR_VALUE_0 { DepthStencil: D3D12_DEPTH_STENCIL_VALUE { Depth: 1.0, Stencil: 0 } },
        };

        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                // Изначальное состояние — DEPTH_WRITE: shadow pass пишет в
                // неё как в DSV на первом же кадре, симметрично
                // `create_depth_stencil` выше.
                D3D12_RESOURCE_STATE_DEPTH_WRITE,
                Some(&clear_value),
                &mut resource,
            )?;

            let resource = resource.ok_or_else(|| {
                eprintln!("[RENDERER] ERROR: Shadow map resource is None!");
                Error::from_hresult(HRESULT(1))
            })?;
            println!("[RENDERER] ✓ Shadow map created");

            Ok(Self {
                resource,
                width: resolution,
                height: resolution,
                // Хранится TYPELESS — `create_dsv`/`create_shadow_srv` ниже
                // сами подставляют конкретный формат для своего вида,
                // НЕ читая `self.format` (в отличие от `create_srv`
                // общего назначения выше, который предполагает, что
                // `self.format` уже является валидным SRV-форматом — для
                // typeless-ресурса это было бы неверно).
                format: DXGI_FORMAT_R32_TYPELESS,
                mip_levels: 1,
            })
        }
    }

    /// Создаёт DSV-вид (для записи глубины) поверх TYPELESS-ресурса —
    /// используется shadow map'ом (см. `create_shadow_map` выше), где
    /// `self.format` — R32_TYPELESS, а не сам по себе валидный DSV-формат.
    pub fn create_dsv(&self, handle: D3D12_CPU_DESCRIPTOR_HANDLE) -> Result<()> {
        let device = crate::get_device()?;
        let desc = D3D12_DEPTH_STENCIL_VIEW_DESC {
            Format: DXGI_FORMAT_D32_FLOAT,
            ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2D,
            Flags: D3D12_DSV_FLAG_NONE,
            Anonymous: D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_DSV { MipSlice: 0 },
            },
        };
        unsafe {
            device.CreateDepthStencilView(&self.resource, Some(&desc), handle);
        }
        Ok(())
    }

    /// Создаёт SRV-вид (для чтения в шейдере) поверх TYPELESS-ресурса —
    /// см. комментарий у `create_shadow_map` про typeless-паттерн. Формат
    /// R32_FLOAT — тот же битовый layout, что и D32_FLOAT, но
    /// интерпретируемый шейдером как обычное float-число (не как depth),
    /// что и нужно для ручного сравнения в PCF-фильтрации.
    pub fn create_shadow_srv(&self, handle: D3D12_CPU_DESCRIPTOR_HANDLE) -> Result<()> {
        let device = crate::get_device()?;
        let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R32_FLOAT,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MostDetailedMip: 0,
                    MipLevels: 1,
                    PlaneSlice: 0,
                    ResourceMinLODClamp: 0.0,
                },
            },
        };
        unsafe {
            device.CreateShaderResourceView(&self.resource, Some(&desc), handle);
        }
        Ok(())
    }

    /// ИСПРАВЛЕНО (Фаза 5 плана по реализму/фонарям, обнаружено при
    /// добавлении bloom-прохода): `RenderTexture` (этот тип) и `Texture`
    /// (отдельный тип в texture.rs) — РАЗНЫЕ структуры с одинаковой формой
    /// полей, но `create_srv`/`create_rtv` были определены ТОЛЬКО у
    /// `Texture`. `Renderer::new()` уже вызывал `hdr_target.create_srv(...)`
    /// на значении типа `RenderTexture` — это был бы E0599 ("no method
    /// named `create_srv` found for struct `RenderTexture`"), то есть
    /// ошибка компиляции всего крейта, обнаруженная бы при первой попытке
    /// собрать движок. Добавляем те же методы сюда — с идентичной
    /// реализацией (поля обеих структур совпадают по смыслу и по типам).
    pub fn create_srv(&self, handle: D3D12_CPU_DESCRIPTOR_HANDLE) -> Result<()> {
        let device = crate::get_device()?;
        let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: self.format,
            ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MostDetailedMip: 0,
                    MipLevels: self.mip_levels,
                    PlaneSlice: 0,
                    ResourceMinLODClamp: 0.0,
                },
            },
        };
        unsafe {
            device.CreateShaderResourceView(&self.resource, Some(&desc), handle);
        }
        Ok(())
    }

    /// См. комментарий у `create_srv` выше — то же самое, для RTV.
    pub fn create_rtv(&self, handle: D3D12_CPU_DESCRIPTOR_HANDLE) -> Result<()> {
        let device = crate::get_device()?;
        unsafe {
            device.CreateRenderTargetView(&self.resource, None, handle);
        }
        Ok(())
    }
}

pub struct Renderer {
    pub back_buffers: Vec<RenderTexture>,
    pub render_target_views: Vec<D3D12_CPU_DESCRIPTOR_HANDLE>,
    pub rtv_heap: ID3D12DescriptorHeap,
    pub dsv_heap: ID3D12DescriptorHeap,
    pub depth_stencil: RenderTexture,
    pub depth_stencil_view: D3D12_CPU_DESCRIPTOR_HANDLE,
    pub rtv_size: u32,
    pub dsv_size: u32,
    pub width: u32,
    pub height: u32,

    // ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям): HDR render target
    // (R16G16B16A16_FLOAT) — основной draw pass теперь рисует СЮДА, а не
    // напрямую в 8-битный (R8G8B8A8_UNORM) back buffer. Раньше цвет из
    // пиксельного шейдера (потенциально >1.0 — например сумма нескольких
    // ярких фонарей) немедленно CLAMP'ился в [0,1] при записи в LDR
    // back buffer, что на практике означает: либо сцена всегда тусклая
    // (если её сделать безопасной от пересвета руками), либо яркие
    // фонари превращаются в плоские белые пятна без деталей — но не
    // может быть одновременно и то, и другое хорошо. HDR target хранит
    // цвет как есть (float, без обрезания), а отдельный composite-проход
    // (см. `compile_tonemap_shader`/`tonemap_pass` в engine/mod.rs) делает
    // экспозицию + ACES-тонмаппинг ОДИН РАЗ, осознанно, вместо неявного
    // обрезания на каждой отдельной операции записи в RT.
    pub hdr_target: RenderTexture,
    pub hdr_rtv: D3D12_CPU_DESCRIPTOR_HANDLE,
    /// Отдельный RTV heap (1 дескриптор) под hdr_rtv — `rtv_heap` выше
    /// рассчитан ровно на `buffer_count` back buffer'ов, без запаса.
    /// Хранится здесь, а не отбрасывается сразу после создания, потому
    /// что `hdr_rtv` (CPU handle) остаётся валиден только пока жив хип,
    /// из которого он выдан.
    pub hdr_rtv_heap: ID3D12DescriptorHeap,
    /// SRV на hdr_target — используется composite/tonemap проходом, чтобы
    /// ПРОЧИТАТЬ то, что основной draw pass только что записал через RTV.
    /// Отдельный CBV/SRV/UAV heap (SHADER_VISIBLE), в отличие от RTV/DSV
    /// heap выше — таково требование D3D12 (RTV/DSV heap не могут быть
    /// shader-visible, SRV должен быть в отдельном виде heap).
    pub srv_uav_heap: ID3D12DescriptorHeap,
    pub hdr_srv_gpu: D3D12_GPU_DESCRIPTOR_HANDLE,
}

impl Renderer {
    pub fn new(width: u32, height: u32, buffer_count: u32) -> Result<Self> {
        println!("[RENDERER] ========== CREATING RENDERER ==========");
        println!("[RENDERER] Width: {}, Height: {}, Buffer count: {}", width, height, buffer_count);

        // ИСПРАВЛЕНО: было `state.device.as_ref().unwrap().clone()` и
        // `state.swap_chain.as_ref().unwrap().clone()`.
        let device = crate::get_device()?;
        println!("[RENDERER] Device obtained");

        let swap_chain = crate::get_swap_chain()?;
        println!("[RENDERER] Swap chain obtained");

        let (rtv_size, dsv_size) = {
            let state = STATE.lock().unwrap();
            (state.rtv_descriptor_size, state.dsv_descriptor_size)
        };
        println!("[RENDERER] RTV size: {}, DSV size: {}", rtv_size, dsv_size);

        println!("[RENDERER] Creating RTV heap...");
        let rtv_heap = crate::heap::DescriptorHeap::create_rtv_heap(buffer_count)?;
        println!("[RENDERER] Creating DSV heap...");
        let dsv_heap = crate::heap::DescriptorHeap::create_dsv_heap(1)?;

        let mut back_buffers = Vec::new();
        let mut render_target_views = Vec::new();

        for i in 0..buffer_count {
            println!("[RENDERER] Getting back buffer {}", i);
            let resource: ID3D12Resource = unsafe { swap_chain.GetBuffer(i)? };
            let texture = RenderTexture {
                resource,
                width,
                height,
                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                mip_levels: 1,
            };
            println!("[RENDERER] Back buffer {} resource obtained", i);

            let handle = crate::heap::DescriptorHeap::get_cpu_handle(&rtv_heap, i, rtv_size);
            unsafe {
                device.CreateRenderTargetView(&texture.resource, None, handle);
            }

            back_buffers.push(texture);
            render_target_views.push(handle);
            println!("[RENDERER] ✓ RTV {} created", i);
        }

        println!("[RENDERER] Creating depth stencil...");
        let depth_stencil = RenderTexture::create_depth_stencil(width, height)?;
        let depth_stencil_view = crate::heap::DescriptorHeap::get_cpu_handle(&dsv_heap, 0, dsv_size);
        // ОБНОВЛЕНО (Фаза 8 плана по реализму/фонарям — volumetric-
        // подсветка): раньше здесь стоял `device.CreateDepthStencilView(
        // &depth_stencil.resource, None, ...)` — `None` в качестве desc
        // валиден ТОЛЬКО когда сам ресурс уже создан с конкретным (не
        // TYPELESS) форматом, откуда рантайм мог бы вывести формат вида
        // автоматически. После перевода `create_depth_stencil` на
        // TYPELESS-ресурс (см. подробное обоснование там) `None` здесь
        // привёл бы к ошибке валидации D3D12 — вид не может быть выведен
        // из типобезразличного формата. Используем `depth_stencil.create_dsv`
        // (тот же метод, что уже существовал для shadow map) — он явно
        // указывает DXGI_FORMAT_D32_FLOAT для DSV-вида.
        depth_stencil.create_dsv(depth_stencil_view)?;
        println!("[RENDERER] ✓ Depth stencil created");

        // ДОБАВЛЕНО (Фаза 5 плана по реализму/фонарям): HDR render target
        // + его RTV (отдельный маленький RTV heap на 1 дескриптор — НЕ
        // переиспользуем `rtv_heap` выше, у него ровно `buffer_count`
        // слотов под back buffer'ы, без запаса) + его SRV в отдельном
        // SHADER_VISIBLE CBV/SRV/UAV heap (`create_cbv_srv_uav_heap` уже
        // существовал в heap.rs, но не был подключён нигде в движке до
        // этой фазы).
        println!("[RENDERER] Creating HDR target...");
        let hdr_target = RenderTexture::create_hdr_target(width, height)?;
        let hdr_rtv_heap = crate::heap::DescriptorHeap::create_rtv_heap(1)?;
        let hdr_rtv = crate::heap::DescriptorHeap::get_cpu_handle(&hdr_rtv_heap, 0, rtv_size);
        unsafe {
            device.CreateRenderTargetView(&hdr_target.resource, None, hdr_rtv);
        }

        let srv_uav_heap = crate::heap::DescriptorHeap::create_cbv_srv_uav_heap(4)?;
        let cbv_srv_uav_size = {
            let state = STATE.lock().unwrap();
            state.cbv_srv_uav_descriptor_size
        };
        let hdr_srv_cpu = crate::heap::DescriptorHeap::get_cpu_handle(&srv_uav_heap, 0, cbv_srv_uav_size);
        let hdr_srv_gpu = crate::heap::DescriptorHeap::get_gpu_handle(&srv_uav_heap, 0, cbv_srv_uav_size);
        hdr_target.create_srv(hdr_srv_cpu)?;
        println!("[RENDERER] ✓ HDR target + RTV + SRV created");

        // ВАЖНО: дескрипторный хип (`hdr_rtv_heap`) должен пережить весь
        // срок жизни любого дескриптора, выданного из него — иначе хип
        // освободится, а `hdr_rtv` (просто CPU-адрес внутри уже
        // уничтоженной памяти хипа) станет висячим указателем. Поэтому
        // `hdr_rtv_heap` хранится в Renderer как обычное поле, а не
        // отбрасывается сразу после `get_cpu_handle` — тот же паттерн, что
        // уже используется для `rtv_heap`/`dsv_heap` выше.
        println!("[RENDERER] ✓ Renderer created successfully");
        Ok(Self {
            back_buffers,
            render_target_views,
            rtv_heap,
            dsv_heap,
            depth_stencil,
            depth_stencil_view,
            rtv_size,
            dsv_size,
            width,
            height,
            hdr_target,
            hdr_rtv,
            hdr_rtv_heap,
            srv_uav_heap,
            hdr_srv_gpu,
        })
    }
}
