// src/device.rs
use windows::core::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Direct3D::{
    D3D_FEATURE_LEVEL_12_0,
    // ДОБАВЛЕНО (тени на второй видеокарте — фаза 1; ОБНОВЛЕНО — теперь
    // используется для ОБОИХ устройств, не только второй карты, см.
    // комментарий у `levels_primary` в `D3D12Device::create()` ниже): ни
    // primary, ни secondary адаптер не обязаны поддерживать 12_0 (старые/
    // слабые карты вроде GT710 — не поддерживают) — оба места кода теперь
    // пробуют уровни по убыванию и берут первый, который реально
    // поддерживается, вместо жёсткого требования конкретного уровня.
    D3D_FEATURE_LEVEL_12_1, D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0,
};
use crate::STATE;

pub struct D3D12Device;

impl D3D12Device {
    pub fn create() -> Result<()> {
        println!("[DEVICE] ========== CREATING D3D12 DEVICE ==========");
        let mut state = STATE.lock().unwrap();

        unsafe {
            // ДОБАВЛЕНО (диагностика реального краша E_INVALIDARG на живой
            // машине, который не удавалось точно локализовать по одному
            // только коду по инструментам этой песочницы): включаем D3D12
            // debug layer ДО создания устройства. Она не меняет поведение
            // рендеринга — только добавляет валидацию API-вызовов и копит
            // подробные текстовые сообщения (какой именно вызов, с каким
            // параметром, нарушил какое именно правило) в ID3D12InfoQueue,
            // которые мы теперь читаем и печатаем в render_frame() при
            // ошибке (см. device_removed_reason()/dump_d3d12_debug_messages()
            // в lib.rs). Без него мы видим только голый HRESULT без деталей
            // — с ним рантайм называет ИМЕННО тот вызов и параметр, что
            // сломался. Не фатально, если недоступно (например на машине
            // не установлены Windows SDK "Graphics Tools" — опциональный
            // компонент Windows, без которого DXGIGetDebugInterface1
            // /D3D12GetDebugInterface вернёт ошибку) — тогда просто
            // продолжаем без валидации, как раньше.
            let mut debug: Option<ID3D12Debug> = None;
            match D3D12GetDebugInterface(&mut debug) {
                Ok(()) => {
                    if let Some(debug) = debug {
                        debug.EnableDebugLayer();
                        println!("[DEVICE] ✓ D3D12 debug layer enabled");

                        // ОТКЛЮЧЕНО ПО УМОЛЧАНИЮ (2026-09-05): GBV на тяжёлой
                        // сцене (сотни ECS-сущностей + 3 каскада теней)
                        // раздувала время первого кадра до нескольких МИНУТ —
                        // GPU выглядел подвисшим для Windows, fence timeout не
                        // успевал восстановиться штатно, и в итоге
                        // зависала/крашилась вся система, а не только процесс.
                        // Баг на кадре 2 (E_INVALIDARG из-за содержимого
                        // root-дескрипторов), ради которого включали GBV, всё
                        // ещё не подтверждён как исправленный.
                        //
                        // ДОБАВЛЕНО (2026-09-05, диагностика на main_car):
                        // включается через переменную окружения ALKASH3D_GBV
                        // вместо жёсткого кода — так можно временно включить
                        // GBV именно на лёгкой сцене (main_car — 21 сущность,
                        // не city-демо/world streaming) для одноразовой
                        // диагностики, не трогая поведение по умолчанию
                        // (main.rs/тяжёлые сцены остаются без GBV).
                        match debug.cast::<ID3D12Debug1>() {
                            Ok(debug1) => {
                                if std::env::var("ALKASH3D_GBV").is_ok() {
                                    // ДОБАВЛЕНО (код-ревью: у ALKASH3D_GBV не было
                                    // НИКАКОЙ защиты от того самого сценария, ради
                                    // предотвращения которого GBV вообще выключили
                                    // по умолчанию — включить её на тяжёлой сцене
                                    // (main.rs — city-демо + world streaming, сотни
                                    // сущностей) и снова подвесить всю машину.
                                    // device.rs создаётся ДО загрузки сцены, поэтому
                                    // реальное число сущностей здесь ещё не
                                    // известно — но известно, КАКОЙ бинарник
                                    // запущен, а лёгкие/тяжёлые демо в этом крейте
                                    // жёстко привязаны к конкретным бинарникам (см.
                                    // main.rs vs main1/main2/main_car). Поэтому
                                    // используем имя исполняемого файла как
                                    // практическую замену проверке размера сцены:
                                    // GBV включается ТОЛЬКО для заведомо лёгких
                                    // демо из этого списка, для всех остальных
                                    // (включая main.rs и любой будущий неизвестный
                                    // бинарник) переменная окружения игнорируется.
                                    const GBV_SAFE_BINARIES: &[&str] =
                                        &["main_car", "main1", "main2", "physics_api_smoke"];
                                    let exe_name = std::env::current_exe()
                                        .ok()
                                        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()));
                                    let is_safe = exe_name
                                        .as_deref()
                                        .map(|n| GBV_SAFE_BINARIES.contains(&n))
                                        .unwrap_or(false);

                                    if is_safe {
                                        debug1.SetEnableGPUBasedValidation(true);
                                        println!("[DEVICE] ⚠ GPU-Based Validation ВКЛЮЧЕНА (ALKASH3D_GBV, бинарник '{}' в списке лёгких сцен) — кадр может выполняться на порядки дольше", exe_name.unwrap_or_default());
                                    } else {
                                        println!(
                                            "[DEVICE] ALKASH3D_GBV установлена, но ИГНОРИРУЕТСЯ: бинарник '{}' не входит в список лёгких сцен {:?} — GBV на тяжёлой сцене уже приводила к зависанию всей машины (см. комментарий выше). Запусти диагностику через один из лёгких демо-бинарников.",
                                            exe_name.unwrap_or_else(|| "<неизвестно>".to_string()),
                                            GBV_SAFE_BINARIES
                                        );
                                    }
                                } else {
                                    println!("[DEVICE] GPU-Based Validation доступен, но отключён по умолчанию (см. комментарий в device.rs — на тяжёлой сцене раздувает кадр до минут и может подвесить систему; установи ALKASH3D_GBV=1 для диагностики на лёгкой сцене)");
                                }
                            }
                            Err(e) => {
                                println!("[DEVICE] ID3D12Debug1 (GPU-Based Validation) недоступен ({:?}) — продолжаем без неё", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("[DEVICE] Debug layer unavailable ({:?}) — продолжаем без неё (не критично)", e);
                }
            }

            // ДОБАВЛЕНО (диагностика воспроизведённого DXGI_ERROR_DEVICE_HUNG
            // на "кадре 2" — см. `dump_dred_report()` в lib.rs): DRED (Device
            // Removed Extended Data), в отличие от GPU-Based Validation выше,
            // ПОЧТИ БЕСПЛАТЕН по кадру (просто ведёт кольцевой журнал команд
            // на GPU-стороне) — можно и нужно держать включённым ВСЕГДА, а не
            // только на "безопасных" лёгких демо. Ничего не меняет в
            // поведении рендера, пока устройство не потеряно; при потере
            // `GetAutoBreadcrumbsOutput`/`GetPageFaultAllocationOutput` в
            // `dump_dred_report()` называют КОНКРЕТНУЮ GPU-команду (draw/
            // barrier/present, по позиции в командном списке) и, при page
            // fault, конкретный виртуальный адрес — то, чего обычный debug
            // layer НЕ даёт при зависании (он видел только итоговый
            // DXGI_ERROR_DEVICE_HUNG без причины, см. `run1_err.log` при
            // верификации фикса гонки в constant buffer 2026-09-08). Как и
            // debug layer, включать нужно СТРОГО до создания устройства.
            let mut dred_settings: Option<ID3D12DeviceRemovedExtendedDataSettings> = None;
            match D3D12GetDebugInterface(&mut dred_settings) {
                Ok(()) => {
                    if let Some(dred_settings) = dred_settings {
                        dred_settings.SetAutoBreadcrumbsEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
                        dred_settings.SetPageFaultEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
                        println!("[DEVICE] ✓ DRED enabled (auto-breadcrumbs + page fault reporting)");
                    }
                }
                Err(e) => {
                    println!("[DEVICE] DRED недоступен ({:?}) — продолжаем без него", e);
                }
            }

            println!("[DEVICE] Creating DXGI factory...");
            let dxgi_factory = CreateDXGIFactory1::<IDXGIFactory4>()?;
            println!("[DEVICE] DXGI factory created");

            // ИСПРАВЛЕНО (выбор GPU при наличии нескольких видеокарт в
            // системе — например основная игровая карта + слабая вторая
            // вроде GT710): раньше здесь брался ПЕРВЫЙ попавшийся
            // аппаратный (не software) адаптер в порядке, в котором его
            // отдаёт `EnumAdapters1` — а этот порядок НЕ гарантированно
            // "от мощной карты к слабой". Он зависит от порядка слотов
            // PCIe/драйвера/того, какая карта выставлена первичной в
            // BIOS. То есть на машине с GT710 движок мог с равной
            // вероятностью выбрать именно её для ВСЕГО рендера — что для
            // движка, чья цель — максимальный реализм графики, было бы
            // тихой деградацией картинки без единого предупреждения.
            //
            // Теперь: собираем ВСЕ аппаратные адаптеры, сортируем по
            // объёму выделенной видеопамяти (DedicatedVideoMemory) по
            // убыванию — это тот же надёжный эвристический критерий,
            // которым пользуется большинство движков (дискретная игровая
            // карта почти всегда на порядок отличается по VRAM от старой/
            // слабой второй карты или встроенной графики), и он не зависит
            // от порядка enumeration. Основной рендер идёт СТРОГО на
            // самой мощной карте.
            println!("[DEVICE] Enumerating adapters...");
            struct AdapterInfo {
                adapter: IDXGIAdapter1,
                name: String,
                vram_mb: u64,
            }
            let mut hardware_adapters: Vec<AdapterInfo> = Vec::new();
            for i in 0.. {
                match dxgi_factory.EnumAdapters1(i) {
                    Ok(adap) => {
                        let desc = adap.GetDesc1()?;
                        let is_software = (desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32)) != 0;
                        let name = String::from_utf16_lossy(&desc.Description)
                            .trim_end_matches('\0')
                            .to_string();
                        let vram_mb = (desc.DedicatedVideoMemory / (1024 * 1024)) as u64;
                        println!(
                            "[DEVICE] Adapter {}: {} - Software: {} - VRAM: {} MB",
                            i, name, is_software, vram_mb
                        );
                        // ИСПРАВЛЕНО (реальный краш на живой машине —
                        // HRESULT 0x887A0004 "уровень не поддерживается"
                        // при инициализации): на системе с проблемным
                        // драйвером одной из карт DXGI может отдать
                        // "Microsoft Basic Render Driver" (системный
                        // software-рендерер, присутствует в списке адаптеров
                        // ВСЕГДА, независимо от реального железа) с флагом
                        // `DXGI_ADAPTER_FLAG_SOFTWARE`, который выставлен
                        // НЕВЕРНО (`is_software == false`) — то есть чисто
                        // по флагу он неотличим от настоящей аппаратной
                        // карты и проходит в `hardware_adapters`. У такого
                        // ложного адаптера VRAM=0, но раньше это не
                        // спасало: если он оказывался ЕДИНСТВЕННЫМ
                        // "аппаратным" адаптером в списке (например —
                        // настоящая дискретная карта на этот момент
                        // недоступна из-за кода 31 в диспетчере устройств),
                        // сортировка по VRAM была не при чём, а сам он мог
                        // попасть в primary просто как единственный
                        // претендент. Хуже того, при наличии ДВУХ таких
                        // ложных записей (что и произошло на практике) одна
                        // из них ошибочно засчитывалась как "вторая
                        // видеокарта". Теперь дополнительно проверяем ИМЯ —
                        // "Microsoft Basic Render Driver" никогда не
                        // считается настоящим hardware-адаптером, вне
                        // зависимости от значения флага `is_software`.
                        let is_basic_render_driver = name == "Microsoft Basic Render Driver";
                        if !is_software && !is_basic_render_driver {
                            hardware_adapters.push(AdapterInfo { adapter: adap, name, vram_mb });
                        }
                    }
                    Err(_) => break,
                }
            }

            // Сортировка по убыванию VRAM — самая мощная карта первая.
            hardware_adapters.sort_by(|a, b| b.vram_mb.cmp(&a.vram_mb));

            if hardware_adapters.len() >= 2 {
                let second = &hardware_adapters[1];
                println!(
                    "[DEVICE] Обнаружена вторая видеокарта: {} ({} MB VRAM) — рендер на ней НЕ идёт, \
                     основной рендер всегда на самой мощной карте выше",
                    second.name, second.vram_mb
                );
                state.secondary_gpu_available = true;
                state.secondary_gpu_name = Some(second.name.clone());
                state.secondary_gpu_vram_mb = second.vram_mb;
            }

            let adapter = if hardware_adapters.is_empty() {
                None
            } else {
                Some(hardware_adapters.remove(0).adapter)
            };

            let adapter = adapter.ok_or_else(|| {
                eprintln!("[DEVICE] ERROR: No suitable adapter found!");
                Error::from_hresult(HRESULT(1))
            })?;
            println!("[DEVICE] Selected primary (most powerful) hardware adapter");

            // ДОБАВЛЕНО (тени на второй видеокарте — фаза 1): после того как
            // основной (самый мощный) адаптер извлечён выше, в
            // `hardware_adapters` остаются только более слабые карты — если
            // там что-то есть, первый элемент и есть "вторая по мощности"
            // карта (та же, для которой чуть выше залогировали имя/VRAM).
            // Сохраняем именно объект адаптера — он понадобится ниже для
            // попытки создать на нём отдельное D3D12-устройство.
            let secondary_adapter: Option<IDXGIAdapter1> = if hardware_adapters.is_empty() {
                None
            } else {
                Some(hardware_adapters.remove(0).adapter)
            };

            println!("[DEVICE] Creating D3D12 device...");
            // ИСПРАВЛЕНО (реальный краш на живой машине — HRESULT
            // 0x887A0004 "Указанный интерфейс устройства или уровень
            // компонента не поддерживается в данной системе" прямо на
            // старте): раньше primary-адаптер жёстко требовал
            // D3D_FEATURE_LEVEL_12_0 без какого-либо fallback — в отличие
            // от secondary-адаптера ниже, который уже пробует уровни по
            // убыванию. Из-за этого ЛЮБАЯ ситуация, когда самой "мощной по
            // VRAM" картой в списке оказывается адаптер без поддержки
            // 12_0 (например — единственная доступная в моменте карта
            // старого поколения вроде GT710/Kepler, или временная
            // ситуация, когда основная карта отвалилась из-за проблемы с
            // драйвером — код 31 в диспетчере устройств — и её место
            // "самой мощной" занял единственный оставшийся адаптер),
            // приводила к падению уже на этапе создания устройства, ещё
            // до окна и рендера. Теперь primary тоже пробует уровни по
            // убыванию — так же, как secondary — и падает только если НИ
            // ОДИН уровень не поддерживается вообще ни на каком доступном
            // адаптере.
            let levels_primary = [
                ("12_1", D3D_FEATURE_LEVEL_12_1),
                ("12_0", D3D_FEATURE_LEVEL_12_0),
                ("11_1", D3D_FEATURE_LEVEL_11_1),
                ("11_0", D3D_FEATURE_LEVEL_11_0),
            ];
            let mut device: Option<ID3D12Device> = None;
            let mut selected_level_name = "";
            for (level_name, level) in levels_primary {
                let mut candidate: Option<ID3D12Device> = None;
                match D3D12CreateDevice(&adapter, level, &mut candidate) {
                    Ok(()) => {
                        if candidate.is_some() {
                            device = candidate;
                            selected_level_name = level_name;
                            break;
                        }
                    }
                    Err(e) => {
                        println!(
                            "[DEVICE] Основная карта не поддерживает feature level {}: {:?}",
                            level_name, e
                        );
                    }
                }
            }
            let device = device.ok_or_else(|| {
                eprintln!("[DEVICE] ERROR: Failed to create device! (ни один feature level не поддержан)");
                Error::from_hresult(HRESULT(1))
            })?;
            println!("[DEVICE] D3D12 device created (feature level {})", selected_level_name);

            println!("[DEVICE] Getting descriptor sizes...");
            let rtv_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
            let dsv_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_DSV);
            let cbv_srv_uav_size = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
            println!("[DEVICE] RTV size: {}, DSV size: {}, CBV/SRV/UAV size: {}",
                     rtv_size, dsv_size, cbv_srv_uav_size);

            // ДОБАВЛЕНО: если debug layer выше реально включился, у самого
            // устройства (не у ID3D12Debug) можно запросить дополнительный
            // интерфейс ID3D12InfoQueue через QueryInterface (`.cast()`) —
            // именно через него читаются накопленные валидационные
            // сообщения. Если debug layer не был включён, `.cast()` здесь
            // тоже вернёт ошибку (интерфейс просто не реализован устройством
            // без debug layer) — это ожидаемо и не критично.
            match device.cast::<ID3D12InfoQueue>() {
                Ok(info_queue) => {
                    state.info_queue = Some(info_queue);
                    println!("[DEVICE] ✓ D3D12 info queue obtained (подробные сообщения об ошибках доступны)");
                }
                Err(_) => {
                    // Тихо — это ожидаемый случай, если debug layer не
                    // включился (см. предупреждение выше), отдельное
                    // сообщение об этом было бы шумом.
                }
            }

            state.device = Some(device);
            state.rtv_descriptor_size = rtv_size;
            state.dsv_descriptor_size = dsv_size;
            state.cbv_srv_uav_descriptor_size = cbv_srv_uav_size;
            println!("[DEVICE] ✓ D3D12 device initialized successfully");

            // ДОБАВЛЕНО (тени на второй видеокарте — фаза 1): пробуем
            // создать ОТДЕЛЬНОЕ D3D12-устройство на второй карте, если она
            // есть. ВАЖНО: это строго дополнительный, изолированный шаг —
            // любая ошибка здесь (нет второй карты, старый драйвер, не
            // поддерживается ни один из проверяемых feature level) только
            // логируется и оставляет `secondary_gpu_usable = false`. Она НЕ
            // должна и не может провалить создание основного устройства
            // выше (оно уже полностью создано и записано в state.device к
            // этому моменту) — весь остальной движок при `false` продолжает
            // работать ровно как без второй карты, ни один существующий
            // путь рендера этот флаг пока не читает.
            if let Some(secondary_adapter) = secondary_adapter {
                println!("[DEVICE] Пробуем создать устройство на второй карте...");

                // GT710 и подобные старые карты обычно поддерживают только
                // feature level 11_0 (Kepler) — в отличие от основной карты,
                // где мы жёстко требуем 12_0. Пробуем от старшего к
                // младшему и берём первый уровень, который реально
                // поддерживается — а не гадаем заранее.
                let levels = [
                    ("12_1", D3D_FEATURE_LEVEL_12_1),
                    ("12_0", D3D_FEATURE_LEVEL_12_0),
                    ("11_1", D3D_FEATURE_LEVEL_11_1),
                    ("11_0", D3D_FEATURE_LEVEL_11_0),
                ];

                let mut secondary_device: Option<ID3D12Device> = None;
                for (level_name, level) in levels {
                    let mut candidate: Option<ID3D12Device> = None;
                    match D3D12CreateDevice(&secondary_adapter, level, &mut candidate) {
                        Ok(()) => {
                            if let Some(dev) = candidate {
                                println!(
                                    "[DEVICE] ✓ Устройство второй карты создано (feature level {})",
                                    level_name
                                );
                                secondary_device = Some(dev);
                                break;
                            }
                        }
                        Err(e) => {
                            println!(
                                "[DEVICE] Вторая карта не поддерживает feature level {}: {:?}",
                                level_name, e
                            );
                        }
                    }
                }

                match secondary_device {
                    Some(dev) => {
                        let queue_desc = D3D12_COMMAND_QUEUE_DESC {
                            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                            Priority: D3D12_COMMAND_QUEUE_PRIORITY_NORMAL.0 as i32,
                            Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
                            NodeMask: 0,
                        };
                        match dev.CreateCommandQueue::<ID3D12CommandQueue>(&queue_desc) {
                            Ok(queue) => {
                                println!("[DEVICE] ✓ Очередь команд второй карты создана — вторая карта готова к использованию");
                                state.secondary_device = Some(dev);
                                state.secondary_command_queue = Some(queue);
                                state.secondary_gpu_usable = true;
                            }
                            Err(e) => {
                                println!(
                                    "[DEVICE] Не удалось создать очередь команд на второй карте ({:?}) — \
                                     вторая карта не будет использоваться, работаем как с одной картой",
                                    e
                                );
                            }
                        }
                    }
                    None => {
                        println!(
                            "[DEVICE] Не удалось создать D3D12-устройство на второй карте ни на одном \
                             feature level — работаем как с одной картой"
                        );
                    }
                }
            }
        }

        Ok(())
    }

    pub fn get() -> Option<ID3D12Device> {
        STATE.lock().unwrap().device.clone()
    }
}