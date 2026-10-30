// src/buffer.rs
use windows::core::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use crate::STATE;

#[derive(Clone)]
pub struct Buffer {
    pub resource: ID3D12Resource,
    pub size: u64,
    pub vertex_stride: u32,
}

/// Копирование в видеопамять, отправленное на GPU: держит всё, что оно читает.
/// Отпускать только после `is_finished` или `wait`.
pub struct PendingCopies {
    fence: ID3D12Fence,
    _allocator: ID3D12CommandAllocator,
    _list: ID3D12GraphicsCommandList,
    _sources: Vec<Buffer>,
}

impl PendingCopies {
    pub fn is_finished(&self) -> bool {
        unsafe { self.fence.GetCompletedValue() >= 1 }
    }

    fn wait(&self) -> Result<()> {
        if self.is_finished() {
            return Ok(());
        }
        unsafe {
            let event = CreateEventW(None, false, false, None)?;
            let waited = self.fence.SetEventOnCompletion(1, event).map(|_| WaitForSingleObject(event, 5000));
            let _ = CloseHandle(event);
            if waited? != WAIT_OBJECT_0 {
                eprintln!("[BUFFER] копирование в видеопамять не завершилось за 5 с");
                return Err(Error::from_hresult(HRESULT(1)));
            }
        }
        Ok(())
    }
}

impl Buffer {
    pub fn create_vertex_buffer(data: &[u8], stride: u32) -> Result<Self> {
        // УБРАНО (оптимизация — жалоба пользователя на лаги/просадки FPS
        // при загрузке новых чанков world streaming): этот println! (и
        // ещё два success-лога ниже — "Data copied successfully",
        // "Vertex buffer created successfully") выполнялись СИНХРОННО на
        // КАЖДЫЙ вызов создания vertex/index-буфера, а `load_chunk`
        // (см. engine/mod.rs) может создавать десятки буферов/текстур за
        // один кадр при входе камеры в новый чанк с несколькими
        // объектами. stdout на Windows, когда подключена консоль, пишет
        // построчно и синхронно (в т.ч. ANSI-парсинг терминала) — пачка
        // из 20-30 println! в одном кадре реально стоит миллисекунды и
        // складывается в заметный фриз в момент загрузки чанка, отдельно
        // от самой GPU-операции создания ресурса. Ошибочные пути
        // (`eprintln!` ниже) — редкие, некритичные по перфомансу,
        // оставлены как есть для диагностики.
        //
        // ИСПРАВЛЕНО: было `state.device.as_ref().unwrap().clone()` —
        // паниковало, если вызвано до инициализации устройства. Теперь
        // явная ошибка через `?`.
        let device = crate::get_device()?;

        let size = data.len() as u64;

        let heap_properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_UPLOAD,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
        };

        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Alignment: 0,
            Width: size,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };

        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            let hr = device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                D3D12_RESOURCE_STATE_GENERIC_READ,
                None,
                &mut resource,
            );
            if hr.is_err() {
                eprintln!("[BUFFER] CreateCommittedResource failed: {:?}", hr);
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            let resource = resource.ok_or_else(|| {
                eprintln!("[BUFFER] Resource is None");
                Error::from_hresult(HRESULT(1))
            })?;

            let mut mapped = std::ptr::null_mut();
            let hr = resource.Map(0, None, Some(&mut mapped));
            if hr.is_err() {
                eprintln!("[BUFFER] Map failed: {:?}", hr);
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            if !mapped.is_null() {
                std::ptr::copy_nonoverlapping(data.as_ptr(), mapped as *mut u8, data.len());
            } else {
                eprintln!("[BUFFER] Mapped pointer is null");
            }
            resource.Unmap(0, None);

            Ok(Self {
                resource,
                size,
                vertex_stride: stride,
            })
        }
    }

    pub fn create_index_buffer(data: &[u32]) -> Result<Self> {
        // См. комментарий в create_vertex_buffer выше — success-лог убран
        // из hot path загрузки чанков по той же причине.
        let bytes: Vec<u8> = data.iter().flat_map(|&x| x.to_le_bytes()).collect();
        Self::create_vertex_buffer(&bytes, 4)
    }

    /// ДОБАВЛЕНО (геометрия в видеопамяти): копии буферов в DEFAULT-куче, в том
    /// же порядке. `create_vertex_buffer` кладёт данные в UPLOAD-кучу — это
    /// оперативная память, GPU читает её через PCIe, и каждая вершина обходится
    /// заметно дороже. Копии с CPU уже не записать (`Mesh::update_vertices`
    /// вернёт ошибку), поэтому — только для неизменяемой геометрии. Состояние
    /// то же, что у исходных (GENERIC_READ). Ждёт конца копирования; при
    /// ошибке исходные буферы остаются в силе.
    pub fn copies_in_video_memory(buffers: &[&Buffer]) -> Result<Vec<Buffer>> {
        let (copies, pending) = Self::copies_in_video_memory_nowait(buffers)?;
        pending.wait()?;
        Ok(copies)
    }

    /// То же без ожидания (чанки стриминга: ждать GPU в кадре — фриз). Копии
    /// можно рисовать сразу: копирование стоит в той же очереди раньше любого
    /// следующего кадра. Исходные буферы живут в `PendingCopies`, пока оно не
    /// закончится.
    pub fn copies_in_video_memory_nowait(buffers: &[&Buffer]) -> Result<(Vec<Buffer>, PendingCopies)> {
        let device = crate::get_device()?;
        let heap_properties = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
        unsafe {
            let allocator: ID3D12CommandAllocator = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
            let list: ID3D12GraphicsCommandList = device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)?;
            let mut copies = Vec::with_capacity(buffers.len());
            let mut barriers = Vec::with_capacity(buffers.len());
            for b in buffers {
                let desc = D3D12_RESOURCE_DESC {
                    Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                    Width: b.size,
                    Height: 1,
                    DepthOrArraySize: 1,
                    MipLevels: 1,
                    Format: DXGI_FORMAT_UNKNOWN,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                    ..Default::default()
                };
                let mut resource: Option<ID3D12Resource> = None;
                device.CreateCommittedResource(&heap_properties, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut resource)?;
                let resource = resource.ok_or_else(|| Error::from_hresult(HRESULT(1)))?;
                list.CopyBufferRegion(&resource, 0, &b.resource, 0, b.size);
                barriers.push(D3D12_RESOURCE_BARRIER {
                    Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
                    Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
                    Anonymous: D3D12_RESOURCE_BARRIER_0 {
                        Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                            pResource: std::mem::ManuallyDrop::new(Some(resource.clone())),
                            Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                            StateBefore: D3D12_RESOURCE_STATE_COPY_DEST,
                            StateAfter: D3D12_RESOURCE_STATE_GENERIC_READ,
                        }),
                    },
                });
                copies.push(Buffer { resource, size: b.size, vertex_stride: b.vertex_stride });
            }
            list.ResourceBarrier(&barriers);
            for barrier in &mut barriers {
                let mut transition = std::mem::ManuallyDrop::take(&mut barrier.Anonymous.Transition);
                std::mem::ManuallyDrop::drop(&mut transition.pResource);
            }
            list.Close()?;

            let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
            let queue = crate::get_command_queue()?;
            let lists: [Option<ID3D12CommandList>; 1] = [Some(list.clone().into())];
            queue.ExecuteCommandLists(&lists);
            queue.Signal(&fence, 1)?;
            let sources = buffers.iter().map(|b| (*b).clone()).collect();
            Ok((copies, PendingCopies { fence, _allocator: allocator, _list: list, _sources: sources }))
        }
    }

    pub fn create_constant_buffer(size: u64) -> Result<Self> {
        println!("[BUFFER] Creating constant buffer, size: {} bytes", size);

        // ИСПРАВЛЕНО: было `state.device.as_ref().unwrap().clone()`.
        let device = crate::get_device()?;

        let aligned_size = (size + 255) & !255;

        let heap_properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_UPLOAD,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
        };

        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Alignment: 0,
            Width: aligned_size,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };

        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            let hr = device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                D3D12_RESOURCE_STATE_GENERIC_READ,
                None,
                &mut resource,
            );
            if hr.is_err() {
                eprintln!("[BUFFER] CreateConstantBuffer failed: {:?}", hr);
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            let resource = resource.ok_or_else(|| Error::from_hresult(HRESULT(1)))?;

            println!("[BUFFER] Constant buffer created successfully");
            Ok(Self {
                resource,
                size: aligned_size,
                vertex_stride: 0,
            })
        }
    }

    /// ДОБАВЛЕНО (Фаза 2 плана по реализму/фонарям): буфер под
    /// `StructuredBuffer<T>` для чтения из шейдера через SRV (register
    /// tN) — используется для передачи списка видимых `GPULight` из
    /// FirstFires в пиксельный шейдер (см. `AlkashEngine::ensure_light_buffer_capacity`
    /// и `render_frame` в engine/mod.rs).
    ///
    /// В отличие от `create_constant_buffer`, здесь НЕТ выравнивания на
    /// 256 байт — это требование D3D12 специфично для CBV
    /// (`D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT`), а не для SRV
    /// structured buffer. Буфер создаётся в UPLOAD heap (то же, что и
    /// вершинный/константный буферы) — для света это оправданно: список
    /// перезаписывается каждый кадр (новый набор видимых после каллинга),
    /// а не создаётся один раз, так что постоянный CPU-доступ через Map
    /// важнее, чем более быстрый, но однократно инициализируемый DEFAULT
    /// heap. Для очень больших городов (тысячи фонарей) это можно будет
    /// пересмотреть в сторону DEFAULT heap + промежуточный upload-буфер,
    /// но на диапазон источников, который считает сам FirstFires
    /// (max_lights в LightConfig), UPLOAD heap достаточно дёшев.
    pub fn create_structured_buffer(size_bytes: u64) -> Result<Self> {
        println!("[BUFFER] Creating structured buffer, size: {} bytes", size_bytes);

        let device = crate::get_device()?;

        // D3D12 не создаёт ресурсы нулевого размера — минимум 1 элемент
        // (вызывающий код всё равно не станет писать в него 0 байт).
        let size = size_bytes.max(4);

        let heap_properties = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_UPLOAD,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
        };

        let resource_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Alignment: 0,
            Width: size,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };

        unsafe {
            let mut resource: Option<ID3D12Resource> = None;
            let hr = device.CreateCommittedResource(
                &heap_properties,
                D3D12_HEAP_FLAG_NONE,
                &resource_desc,
                D3D12_RESOURCE_STATE_GENERIC_READ,
                None,
                &mut resource,
            );
            if hr.is_err() {
                eprintln!("[BUFFER] CreateStructuredBuffer failed: {:?}", hr);
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            let resource = resource.ok_or_else(|| Error::from_hresult(HRESULT(1)))?;

            println!("[BUFFER] Structured buffer created successfully");
            Ok(Self {
                resource,
                size,
                vertex_stride: 0,
            })
        }
    }

    /// Записывает данные в structured buffer целиком (аналог
    /// `update_constant_buffer`, но без ограничения `self.size` под
    /// constant-buffer alignment — просто копирует ровно `data.len()`
    /// байт, либо меньше `self.size`, если данных больше, чем выделено).
    pub fn update_structured_buffer(&self, data: &[u8]) -> Result<()> {
        unsafe {
            let mut mapped = std::ptr::null_mut();
            self.resource.Map(0, None, Some(&mut mapped))?;
            if !mapped.is_null() {
                let len = data.len().min(self.size as usize);
                if len < data.len() {
                    eprintln!(
                        "[BUFFER] WARNING: update_structured_buffer: {} bytes переданы, но буфер вмещает только {} — обрезаю",
                        data.len(), self.size
                    );
                }
                std::ptr::copy_nonoverlapping(data.as_ptr(), mapped as *mut u8, len);
            } else {
                eprintln!("[BUFFER] WARNING: update_structured_buffer: mapped pointer is null, data NOT copied");
            }
            self.resource.Unmap(0, None);
        }
        Ok(())
    }

    /// Создаёт константный буфер, вмещающий несколько независимых "слотов"
    /// одинакового размера — по одному слоту на каждый объект, который
    /// нужно отрисовать в кадре со своей собственной трансформацией. См.
    /// подробное объяснение, зачем это нужно, в `TransformConstants::write_at`.
    pub fn create_constant_buffer_array(slot_aligned_size: u64, slot_count: usize) -> Result<Self> {
        println!(
            "[BUFFER] Creating constant buffer ARRAY: {} slots x {} bytes = {} bytes total",
            slot_count, slot_aligned_size, slot_aligned_size * slot_count as u64
        );
        Self::create_constant_buffer(slot_aligned_size * slot_count as u64)
    }

    pub fn update_constant_buffer(&self, data: &[u8]) -> Result<()> {
        unsafe {
            let mut mapped = std::ptr::null_mut();
            // ИСПРАВЛЕНО: раньше ошибка Map() тут молча проглатывалась
            // (`let _ = self.resource.Map(...)`) — если маппинг не удался,
            // мы бы просто ничего не скопировали и не узнали об этом.
            // Теперь ошибка распространяется наружу через `?`.
            self.resource.Map(0, None, Some(&mut mapped))?;
            if !mapped.is_null() {
                std::ptr::copy_nonoverlapping(data.as_ptr(), mapped as *mut u8, data.len().min(self.size as usize));
            } else {
                eprintln!("[BUFFER] WARNING: update_constant_buffer: mapped pointer is null, data NOT copied");
            }
            self.resource.Unmap(0, None);
        }
        Ok(())
    }

    /// То же самое, что `update_constant_buffer`, но пишет данные по
    /// смещению `offset` внутри буфера, а не в его начало — нужно для
    /// работы со "слотовым" буфером из `create_constant_buffer_array`.
    pub fn update_constant_buffer_at(&self, offset: u64, data: &[u8]) -> Result<()> {
        unsafe {
            let mut mapped = std::ptr::null_mut();
            self.resource.Map(0, None, Some(&mut mapped))?;
            if !mapped.is_null() {
                let max_len = self.size.saturating_sub(offset) as usize;
                let len = data.len().min(max_len);
                if len < data.len() {
                    eprintln!(
                        "[BUFFER] WARNING: update_constant_buffer_at: offset {} + data {} bytes overflows buffer of size {}, truncating",
                        offset, data.len(), self.size
                    );
                }
                let dst = (mapped as *mut u8).add(offset as usize);
                std::ptr::copy_nonoverlapping(data.as_ptr(), dst, len);
            } else {
                eprintln!("[BUFFER] WARNING: update_constant_buffer_at: mapped pointer is null, data NOT copied");
            }
            self.resource.Unmap(0, None);
        }
        Ok(())
    }
}
