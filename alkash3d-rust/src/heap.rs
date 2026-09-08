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

    /// ДОБАВЛЕНО (occlusion culling на второй карте): точная копия
    /// `create_dsv_heap`, но с явно переданным `device` вместо жёстко
    /// зашитого `crate::get_device()` (первая карта) — descriptor heap,
    /// как и всё остальное в D3D12, привязан к конкретному устройству,
    /// поэтому под вторую карту нужен отдельный вызов CreateDescriptorHeap
    /// именно на её device.
    pub fn create_dsv_heap_on_device(device: &ID3D12Device, count: u32) -> Result<ID3D12DescriptorHeap, windows::core::Error> {
        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
            NumDescriptors: count,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
            NodeMask: 0,
        };
        unsafe {
            let heap = device.CreateDescriptorHeap(&desc)?;
            println!("[HEAP] ✓ DSV heap created (вторая карта)");
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

    // ИСПРАВЛЕНО (найдено при аудите — жалоба пользователя на низкий FPS):
    // `get_cpu_handle`/`get_gpu_handle` вызываются КАЖДЫЙ КАДР для КАЖДОГО
    // объекта/материала в основном цикле рендера (в отличие от
    // create_*_heap выше, которые выполняются один раз при инициализации
    // — там println! оставлены, это не hot path). `println!` здесь means
    // форматирование строки + системный вызов записи в консоль на КАЖДЫЙ
    // такой вызов, десятки-сотни раз за кадр — то есть тысячи операций
    // ввода-вывода в секунду только на этот лог, который к тому же после
    // добавления console_log.rs дополнительно каждый раз пишется ещё и в
    // файл на диск (см. engine_log.txt — вырос до десятков мегабайт всего
    // за пару минут работы именно из-за этого спама). Сама диагностическая
    // ценность этих строк крайне мала (просто index/offset без контекста
    // "для какого объекта", "на каком кадре") — убраны полностью.
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
