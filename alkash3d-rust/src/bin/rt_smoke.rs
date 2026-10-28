//! rt_smoke — проверка фундамента непрямого света лучами (DDGI) на этой
//! машине, без окна и без движка: D3D12-устройство с DXR 1.1, DXC (SM 6.5),
//! BLAS одного треугольника, TLAS, compute-шейдер с inline `RayQuery`.
//! Четыре луча вдоль +Z: два должны попасть в треугольник (t = 1), два —
//! мимо. Код выхода 0 — всё совпало.

use std::mem::ManuallyDrop;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const SHADER: &str = r#"
RaytracingAccelerationStructure Scene : register(t0);
RWStructuredBuffer<float4> Out : register(u0);

static const float2 XY[4] = { float2(0, 0), float2(0.9, 0.9), float2(0, -0.5), float2(5, 5) };

[numthreads(4, 1, 1)]
void main(uint id : SV_DispatchThreadID) {
    RayDesc ray;
    ray.Origin = float3(XY[id], -1.0);
    ray.Direction = float3(0, 0, 1);
    ray.TMin = 0.0;
    ray.TMax = 100.0;
    RayQuery<RAY_FLAG_CULL_NON_OPAQUE | RAY_FLAG_SKIP_PROCEDURAL_PRIMITIVES> q;
    q.TraceRayInline(Scene, RAY_FLAG_NONE, 0xFF, ray);
    q.Proceed();
    if (q.CommittedStatus() == COMMITTED_TRIANGLE_HIT) {
        Out[id] = float4(1.0, q.CommittedRayT(), q.CommittedTriangleBarycentrics());
    } else {
        Out[id] = float4(0.0, -1.0, 0.0, 0.0);
    }
}
"#;

fn buffer(device: &ID3D12Device5, size: u64, heap: D3D12_HEAP_TYPE, flags: D3D12_RESOURCE_FLAGS, state: D3D12_RESOURCE_STATES) -> Result<ID3D12Resource> {
    let props = D3D12_HEAP_PROPERTIES { Type: heap, ..Default::default() };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: size.max(256),
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        Flags: flags,
        ..Default::default()
    };
    let mut r: Option<ID3D12Resource> = None;
    unsafe { device.CreateCommittedResource(&props, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut r)? };
    Ok(r.unwrap())
}

fn upload<T: Copy>(device: &ID3D12Device5, data: &[T]) -> Result<ID3D12Resource> {
    let size = std::mem::size_of_val(data) as u64;
    let r = buffer(device, size, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_GENERIC_READ)?;
    unsafe {
        let mut p = std::ptr::null_mut();
        r.Map(0, None, Some(&mut p))?;
        std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, p as *mut u8, size as usize);
        r.Unmap(0, None);
    }
    Ok(r)
}

fn uav_barrier(list: &ID3D12GraphicsCommandList4, r: &ID3D12Resource) {
    let b = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 { UAV: ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER { pResource: ManuallyDrop::new(Some(r.clone())) }) },
    };
    unsafe {
        list.ResourceBarrier(&[b.clone()]);
        let mut b = b;
        ManuallyDrop::drop(&mut (*b.Anonymous.UAV).pResource);
    }
}

/// Построить структуру ускорения: размеры, буфер результата, scratch, запись в список.
fn build_as(device: &ID3D12Device5, list: &ID3D12GraphicsCommandList4, inputs: &D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS, keep: &mut Vec<ID3D12Resource>) -> Result<ID3D12Resource> {
    let mut info = D3D12_RAYTRACING_ACCELERATION_STRUCTURE_PREBUILD_INFO::default();
    unsafe { device.GetRaytracingAccelerationStructurePrebuildInfo(inputs, &mut info) };
    let result = buffer(device, info.ResultDataMaxSizeInBytes, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_RAYTRACING_ACCELERATION_STRUCTURE)?;
    let scratch = buffer(device, info.ScratchDataSizeInBytes, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_UNORDERED_ACCESS)?;
    unsafe {
        let desc = D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_DESC {
            DestAccelerationStructureData: result.GetGPUVirtualAddress(),
            Inputs: *inputs,
            SourceAccelerationStructureData: 0,
            ScratchAccelerationStructureData: scratch.GetGPUVirtualAddress(),
        };
        list.BuildRaytracingAccelerationStructure(&desc, None);
    }
    uav_barrier(list, &result);
    keep.push(scratch);
    Ok(result)
}

fn run() -> std::result::Result<(), String> {
    let e = |m: &str| { let m = m.to_string(); move |err: Error| format!("{}: {}", m, err) };
    unsafe {
        let mut device: Option<ID3D12Device5> = None;
        D3D12CreateDevice(None, D3D_FEATURE_LEVEL_12_1, &mut device).map_err(e("D3D12CreateDevice"))?;
        let device = device.unwrap();

        let mut o5 = D3D12_FEATURE_DATA_D3D12_OPTIONS5::default();
        device.CheckFeatureSupport(D3D12_FEATURE_D3D12_OPTIONS5, &mut o5 as *mut _ as *mut _, std::mem::size_of_val(&o5) as u32).map_err(e("OPTIONS5"))?;
        let mut sm = D3D12_FEATURE_DATA_SHADER_MODEL { HighestShaderModel: D3D_SHADER_MODEL_6_6 };
        device.CheckFeatureSupport(D3D12_FEATURE_SHADER_MODEL, &mut sm as *mut _ as *mut _, std::mem::size_of_val(&sm) as u32).map_err(e("SHADER_MODEL"))?;
        println!("[RT] Raytracing tier: {:?}, shader model: 0x{:x}", o5.RaytracingTier, sm.HighestShaderModel.0);
        if o5.RaytracingTier.0 < D3D12_RAYTRACING_TIER_1_1.0 || sm.HighestShaderModel.0 < D3D_SHADER_MODEL_6_5.0 {
            return Err("нужны DXR 1.1 и SM 6.5".into());
        }

        let cs = alkash3d_rs::dxc::compile(SHADER, "cs_6_5", "main").map_err(e("DXC"))?;

        // Root signature: t0 — TLAS (root SRV), u0 — выход (root UAV).
        let params = [
            D3D12_ROOT_PARAMETER { ParameterType: D3D12_ROOT_PARAMETER_TYPE_SRV, Anonymous: D3D12_ROOT_PARAMETER_0 { Descriptor: D3D12_ROOT_DESCRIPTOR { ShaderRegister: 0, RegisterSpace: 0 } }, ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL },
            D3D12_ROOT_PARAMETER { ParameterType: D3D12_ROOT_PARAMETER_TYPE_UAV, Anonymous: D3D12_ROOT_PARAMETER_0 { Descriptor: D3D12_ROOT_DESCRIPTOR { ShaderRegister: 0, RegisterSpace: 0 } }, ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL },
        ];
        let rs_desc = D3D12_ROOT_SIGNATURE_DESC { NumParameters: 2, pParameters: params.as_ptr(), ..Default::default() };
        let mut blob: Option<ID3DBlob> = None;
        D3D12SerializeRootSignature(&rs_desc, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, None).map_err(e("root signature"))?;
        let blob = blob.unwrap();
        let root: ID3D12RootSignature = device.CreateRootSignature(0, std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())).map_err(e("CreateRootSignature"))?;
        let mut pso_desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: ManuallyDrop::new(Some(root.clone())),
            CS: D3D12_SHADER_BYTECODE { pShaderBytecode: cs.data.as_ptr() as *const _, BytecodeLength: cs.data.len() },
            ..Default::default()
        };
        let pso: Result<ID3D12PipelineState> = device.CreateComputePipelineState(&pso_desc);
        ManuallyDrop::drop(&mut pso_desc.pRootSignature);
        let pso = pso.map_err(e("compute PSO"))?;

        let queue: ID3D12CommandQueue = device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() }).map_err(e("queue"))?;
        let alloc: ID3D12CommandAllocator = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT).map_err(e("allocator"))?;
        let list: ID3D12GraphicsCommandList4 = device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &alloc, None).map_err(e("command list"))?;

        // Треугольник в плоскости z = 0.
        let verts: [f32; 9] = [-1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 0.0, 1.0, 0.0];
        let vb = upload(&device, &verts).map_err(e("vertex buffer"))?;
        let mut keep = Vec::new();
        let geom = D3D12_RAYTRACING_GEOMETRY_DESC {
            Type: D3D12_RAYTRACING_GEOMETRY_TYPE_TRIANGLES,
            Flags: D3D12_RAYTRACING_GEOMETRY_FLAG_OPAQUE,
            Anonymous: D3D12_RAYTRACING_GEOMETRY_DESC_0 {
                Triangles: D3D12_RAYTRACING_GEOMETRY_TRIANGLES_DESC {
                    Transform3x4: 0,
                    IndexFormat: DXGI_FORMAT_UNKNOWN,
                    VertexFormat: DXGI_FORMAT_R32G32B32_FLOAT,
                    IndexCount: 0,
                    VertexCount: 3,
                    IndexBuffer: 0,
                    VertexBuffer: D3D12_GPU_VIRTUAL_ADDRESS_AND_STRIDE { StartAddress: vb.GetGPUVirtualAddress(), StrideInBytes: 12 },
                },
            },
        };
        let blas = build_as(&device, &list, &D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS {
            Type: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_TYPE_BOTTOM_LEVEL,
            Flags: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BUILD_FLAG_PREFER_FAST_TRACE,
            NumDescs: 1,
            DescsLayout: D3D12_ELEMENTS_LAYOUT_ARRAY,
            Anonymous: D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS_0 { pGeometryDescs: &geom },
        }, &mut keep).map_err(e("BLAS"))?;

        let instance = D3D12_RAYTRACING_INSTANCE_DESC {
            Transform: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            _bitfield1: 0xFF << 24,
            _bitfield2: 0,
            AccelerationStructure: blas.GetGPUVirtualAddress(),
        };
        let ib = upload(&device, &[instance]).map_err(e("instance buffer"))?;
        let tlas = build_as(&device, &list, &D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS {
            Type: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_TYPE_TOP_LEVEL,
            Flags: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_BUILD_FLAG_PREFER_FAST_TRACE,
            NumDescs: 1,
            DescsLayout: D3D12_ELEMENTS_LAYOUT_ARRAY,
            Anonymous: D3D12_BUILD_RAYTRACING_ACCELERATION_STRUCTURE_INPUTS_0 { InstanceDescs: ib.GetGPUVirtualAddress() },
        }, &mut keep).map_err(e("TLAS"))?;

        let out = buffer(&device, 64, D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS, D3D12_RESOURCE_STATE_UNORDERED_ACCESS).map_err(e("out"))?;
        let readback = buffer(&device, 64, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_FLAG_NONE, D3D12_RESOURCE_STATE_COPY_DEST).map_err(e("readback"))?;
        list.SetComputeRootSignature(&root);
        list.SetPipelineState(&pso);
        list.SetComputeRootShaderResourceView(0, tlas.GetGPUVirtualAddress());
        list.SetComputeRootUnorderedAccessView(1, out.GetGPUVirtualAddress());
        list.Dispatch(1, 1, 1);
        let to_copy = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: ManuallyDrop::new(Some(out.clone())),
                    Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                    StateBefore: D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                    StateAfter: D3D12_RESOURCE_STATE_COPY_SOURCE,
                }),
            },
        };
        list.ResourceBarrier(&[to_copy.clone()]);
        let mut to_copy = to_copy;
        ManuallyDrop::drop(&mut (*to_copy.Anonymous.Transition).pResource);
        list.CopyResource(&readback, &out);
        list.Close().map_err(e("Close"))?;
        queue.ExecuteCommandLists(&[Some(list.cast::<ID3D12CommandList>().map_err(e("cast"))?)]);

        // Ожидание с таймаутом: зависание GPU не должно вешать процесс.
        let fence: ID3D12Fence = device.CreateFence(0, D3D12_FENCE_FLAG_NONE).map_err(e("fence"))?;
        queue.Signal(&fence, 1).map_err(e("Signal"))?;
        let start = std::time::Instant::now();
        while fence.GetCompletedValue() < 1 {
            if start.elapsed().as_secs() > 5 {
                return Err(format!("GPU не ответил за 5 с (removed reason: {:?})", device.GetDeviceRemovedReason()));
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        let mut p = std::ptr::null_mut();
        readback.Map(0, None, Some(&mut p)).map_err(e("Map"))?;
        let res = std::slice::from_raw_parts(p as *const [f32; 4], 4).to_vec();
        readback.Unmap(0, None);
        let expect = [true, false, true, false];
        let mut ok = true;
        for (i, (r, hit)) in res.iter().zip(expect).enumerate() {
            let good = (r[0] > 0.5) == hit && (!hit || (r[1] - 1.0).abs() < 1e-4);
            println!("[RT] луч {}: hit={} t={:.4} bary=({:.3}, {:.3}) {}", i, r[0], r[1], r[2], r[3], if good { "OK" } else { "НЕВЕРНО" });
            ok &= good;
        }
        if ok { Ok(()) } else { Err("результаты лучей не совпали".into()) }
    }
}

fn main() {
    match run() {
        Ok(()) => println!("[RT] PASS: DXR 1.1 + DXC + RayQuery работают"),
        Err(m) => {
            eprintln!("[RT] FAIL: {}", m);
            std::process::exit(1);
        }
    }
}
