// src/heap.rs
use windows::Win32::Graphics::Direct3D12::*;
use crate::STATE;

pub struct DescriptorHeap;

impl DescriptorHeap {
    pub fn create_rtv_heap(count: u32) -> Result<ID3D12DescriptorHeap, windows::core::Error> {
        println!("[HEAP] Creating RTV heap with {} descriptors", count);
        // ИСПРАВЛЕНО: было `state.device.as_ref().unwrap()`.
        let device = crate::get_device()?;

        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            NumDescriptors: count,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
            NodeMask: 0,
        };

        unsafe {
            let heap = device.CreateDescriptorHeap(&desc)?;
            println!("[HEAP] ✓ RTV heap created");
            Ok(heap)
        }
    }

    pub fn create_dsv_heap(count: u32) -> Result<ID3D12DescriptorHeap, windows::core::Error> {
        println!("[HEAP] Creating DSV heap with {} descriptors", count);
        // ИСПРАВЛЕНО: было `state.device.as_ref().unwrap()`.
        let device = crate::get_device()?;

        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
            NumDescriptors: count,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
            NodeMask: 0,
        };

        unsafe {
            let heap = device.CreateDescriptorHeap(&desc)?;
            println!("[HEAP] ✓ DSV heap created");
            Ok(heap)
        }
    }

    pub fn create_cbv_srv_uav_heap(count: u32) -> Result<ID3D12DescriptorHeap, windows::core::Error> {
        println!("[HEAP] Creating CBV/SRV/UAV heap with {} descriptors", count);
        // ИСПРАВЛЕНО: было `state.device.as_ref().unwrap()`.
        let device = crate::get_device()?;

        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: count,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            NodeMask: 0,
        };

        unsafe {
            let heap = device.CreateDescriptorHeap(&desc)?;
            println!("[HEAP] ✓ CBV/SRV/UAV heap created");
            Ok(heap)
        }
    }

    // ИСПРАВЛЕНО (баг "просадка FPS до ~13 после подключения текстур",
    // найдено и подтверждено на реальной машине пользователя): у
    // `get_cpu_handle`/`get_gpu_handle` раньше был `println!` на КАЖДЫЙ
    // вызов. Это было безобидно, пока обе функции вызывались только в
    // редких "setup"-событиях (создание/пересоздание хипа при инициализации
    // или resize окна — единицы раз за всю сессию). Задача #15 (текстуры и
    // PBR-материалы) добавила вызов `get_gpu_handle` в ГОРЯЧИЙ путь —
    // ОДИН раз на КАЖДЫЙ рисуемый меш КАЖДЫЙ кадр (см. render_frame,
    // биндинг albedo-текстуры перед Draw), то есть десятки вызовов на
    // кадр вместо единиц за сессию. Синхронный вывод в консоль (особенно
    // на Windows, особенно если консоль реально видна) — один из самых
    // медленных доступных примитивов, на порядки медленнее самого GPU-вызова,
    // который он якобы просто логирует; при десятках мешей в кадре это и
    // даёт просадку до ~13 FPS. Убрано из ОБЕИХ функций — не только из
    // get_gpu_handle (единственной, реально попавшей в горячий путь), но и
    // из get_cpu_handle тоже, т.к. это тот же класс функции и тот же риск
    // при следующем расширении (например normal/metallic-roughness карты
    // потребуют доп. вызовов get_cpu_handle на каждую текстуру).
    pub fn get_cpu_handle(heap: &ID3D12DescriptorHeap, index: u32, increment_size: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        unsafe {
            let handle = heap.GetCPUDescriptorHandleForHeapStart();
            let offset = (index as u64) * (increment_size as u64);
            let ptr = handle.ptr + offset as usize;
            D3D12_CPU_DESCRIPTOR_HANDLE { ptr }
        }
    }

    pub fn get_gpu_handle(heap: &ID3D12DescriptorHeap, index: u32, increment_size: u32) -> D3D12_GPU_DESCRIPTOR_HANDLE {
        unsafe {
            let handle = heap.GetGPUDescriptorHandleForHeapStart();
            let offset = (index as u64) * (increment_size as u64);
            let ptr = handle.ptr + offset;
            D3D12_GPU_DESCRIPTOR_HANDLE { ptr }
        }
    }
}
