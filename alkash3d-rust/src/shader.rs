//! Шейдерная система с поддержкой встроенных и внешних шейдеров

use std::ffi::c_void;
use std::path::Path;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows_core::*;
use crate::debug_println;

#[link(name = "d3dcompiler")]
extern "system" {
    fn D3DCompile(
        pSrcData: *const core::ffi::c_void,
        SrcDataSize: usize,
        pSourceName: PCSTR,
        pDefines: *const D3D_SHADER_MACRO,
        pInclude: *const core::ffi::c_void,
        pEntrypoint: PCSTR,
        pTarget: PCSTR,
        Flags1: u32,
        Flags2: u32,
        ppCode: *mut *mut c_void,
        ppErrorMsgs: *mut *mut c_void,
    ) -> HRESULT;
}

#[repr(C)]
struct D3D_SHADER_MACRO {
    name: PCSTR,
    Definition: PCSTR,
}

// Базовый вершинный шейдер (позиция + цвет)
const VS_SIMPLE: &str = r#"
struct VSInput {
    float3 position : POSITION;
    float4 color : COLOR;
};

struct VSOutput {
    float4 position : SV_POSITION;
    float4 color : COLOR;
};

cbuffer ConstantBuffer : register(b0) {
    float4x4 mvp;
};

VSOutput main(VSInput input) {
    VSOutput output;
    output.position = mul(float4(input.position, 1.0), mvp);
    output.color = input.color;
    return output;
}
"#;

// Базовый пиксельный шейдер
const PS_SIMPLE: &str = r#"
struct PSInput {
    float4 position : SV_POSITION;
    float4 color : COLOR;
};

float4 main(PSInput input) : SV_TARGET {
    return input.color;
}
"#;

// Расширенный вершинный шейдер с освещением
const VS_ADVANCED: &str = r#"
struct VSInput {
    float3 position : POSITION;
    float3 normal : NORMAL;
    float3 tangent : TANGENT;
    float3 bitangent : BITANGENT;
    float2 uv : TEXCOORD0;
    float2 uv2 : TEXCOORD1;
    float4 color : COLOR;
};

struct VSOutput {
    float4 position : SV_POSITION;
    float3 worldPos : WORLDPOS;
    float3 normal : NORMAL;
    float2 uv : TEXCOORD0;
    float4 color : COLOR;
};

cbuffer ConstantBuffer : register(b0) {
    float4x4 world;
    float4x4 view;
    float4x4 proj;
    float4 tintColor;
    float3 lightDir;
    float lightIntensity;
    float3 ambientColor;
    float ambientIntensity;
};

VSOutput main(VSInput input) {
    VSOutput output;

    float4 worldPos = mul(float4(input.position, 1.0), world);
    output.worldPos = worldPos.xyz;
    output.position = mul(worldPos, view);
    output.position = mul(output.position, proj);

    output.normal = normalize(mul(float4(input.normal, 0.0), world).xyz);
    output.uv = input.uv;
    output.color = input.color * tintColor;

    return output;
}
"#;

// Расширенный пиксельный шейдер с освещением
const PS_ADVANCED: &str = r#"
struct PSInput {
    float4 position : SV_POSITION;
    float3 worldPos : WORLDPOS;
    float3 normal : NORMAL;
    float2 uv : TEXCOORD0;
    float4 color : COLOR;
};

cbuffer ConstantBuffer : register(b0) {
    float4x4 world;
    float4x4 view;
    float4x4 proj;
    float4 tintColor;
    float3 lightDir;
    float lightIntensity;
    float3 ambientColor;
    float ambientIntensity;
};

float4 main(PSInput input) : SV_TARGET {
    float3 normal = normalize(input.normal);
    float3 lightDirNorm = normalize(-lightDir);

    float diff = max(dot(normal, lightDirNorm), 0.0);
    float3 diffuse = float3(1.0, 1.0, 1.0) * diff * lightIntensity;
    float3 ambient = ambientColor * ambientIntensity;

    float3 finalColor = input.color.rgb * (ambient + diffuse);

    return float4(finalColor, input.color.a);
}
"#;

#[repr(u32)]
pub enum ShaderType {
    Simple = 0,
    Advanced = 1,
    Custom = 2,
}

#[no_mangle]
pub extern "C" fn compile_shader_from_source(
    source: *const i8,
    entry_point: *const i8,
    target: *const i8,
) -> *mut c_void {
    unsafe {
        if source.is_null() || entry_point.is_null() || target.is_null() {
            return std::ptr::null_mut();
        }

        let source_str = std::ffi::CStr::from_ptr(source).to_string_lossy();
        let entry_str = std::ffi::CStr::from_ptr(entry_point).to_string_lossy();
        let target_str = std::ffi::CStr::from_ptr(target).to_string_lossy();

        compile_shader_internal(&source_str, &target_str, &entry_str)
    }
}

#[no_mangle]
pub extern "C" fn compile_shader_from_file(
    file_path: *const i8,
    entry_point: *const i8,
    target: *const i8,
) -> *mut c_void {
    unsafe {
        if file_path.is_null() {
            return std::ptr::null_mut();
        }

        let path_str = std::ffi::CStr::from_ptr(file_path).to_string_lossy();
        let path = Path::new(path_str.as_ref());

        match std::fs::read_to_string(path) {
            Ok(source) => {
                let entry_str = if entry_point.is_null() {
                    "main".to_string()
                } else {
                    std::ffi::CStr::from_ptr(entry_point).to_string_lossy().to_string()
                };
                let target_str = if target.is_null() {
                    "vs_5_0".to_string()
                } else {
                    std::ffi::CStr::from_ptr(target).to_string_lossy().to_string()
                };

                compile_shader_internal(&source, &target_str, &entry_str)
            }
            Err(e) => {
                debug_println!("[compile_shader] Failed to read file: {:?}", e);
                std::ptr::null_mut()
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn get_builtin_shader(shader_type: u32, is_vertex: bool) -> *mut c_void {
    match shader_type {
        0 => { // Simple
            if is_vertex {
                compile_shader_internal(VS_SIMPLE, "vs_5_0", "main")
            } else {
                compile_shader_internal(PS_SIMPLE, "ps_5_0", "main")
            }
        }
        1 => { // Advanced
            if is_vertex {
                compile_shader_internal(VS_ADVANCED, "vs_5_0", "main")
            } else {
                compile_shader_internal(PS_ADVANCED, "ps_5_0", "main")
            }
        }
        _ => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn get_builtin_vs_blob() -> *mut c_void {
    get_builtin_shader(0, true)
}

#[no_mangle]
pub extern "C" fn get_builtin_ps_blob() -> *mut c_void {
    get_builtin_shader(0, false)
}

#[no_mangle]
pub extern "C" fn get_advanced_vs_blob() -> *mut c_void {
    get_builtin_shader(1, true)
}

#[no_mangle]
pub extern "C" fn get_advanced_ps_blob() -> *mut c_void {
    get_builtin_shader(1, false)
}

fn compile_shader_internal(source: &str, target: &str, entry: &str) -> *mut c_void {
    let entry_cstr = std::ffi::CString::new(entry).unwrap();
    let target_cstr = std::ffi::CString::new(target).unwrap();

    let mut code_blob: *mut c_void = std::ptr::null_mut();
    let mut error_blob: *mut c_void = std::ptr::null_mut();

    // Флаги компиляции - используем 0 для минимальной компиляции без отладки
    let compile_flags = 0;

    unsafe {
        let hr = D3DCompile(
            source.as_ptr() as *const core::ffi::c_void,
            source.len(),
            PCSTR(b"shader.hlsl\0".as_ptr()),
            std::ptr::null(),
            std::ptr::null(),
            PCSTR(entry_cstr.as_ptr() as *const u8),
            PCSTR(target_cstr.as_ptr() as *const u8),
            compile_flags,
            0,
            &mut code_blob,
            &mut error_blob,
        );

        if hr.is_err() {
            if !error_blob.is_null() {
                let error_interface: ID3DBlob = std::mem::transmute(error_blob);
                let err_ptr = error_interface.GetBufferPointer();
                let err_size = error_interface.GetBufferSize();
                if err_size > 0 && !err_ptr.is_null() {
                    let err_msg = std::slice::from_raw_parts(err_ptr as *const u8, err_size);
                    debug_println!("Shader compilation error:\n{}", String::from_utf8_lossy(err_msg));
                }
                std::mem::forget(error_interface);
            } else {
                debug_println!("Shader compilation failed with HRESULT: {:?}", hr);
            }
            return std::ptr::null_mut();
        }

        if code_blob.is_null() {
            debug_println!("Shader compilation succeeded but blob is null");
            return std::ptr::null_mut();
        }

        let blob_check: ID3DBlob = std::mem::transmute(code_blob);
        let size = blob_check.GetBufferSize();
        std::mem::forget(blob_check);

        debug_println!("✅ Shader compiled successfully ({} bytes)", size);
        code_blob
    }
}

#[no_mangle]
pub extern "C" fn get_blob_size(blob_ptr: *mut c_void) -> usize {
    if blob_ptr.is_null() {
        return 0;
    }
    unsafe {
        let blob: ID3DBlob = std::mem::transmute(blob_ptr);
        let size = blob.GetBufferSize();
        std::mem::forget(blob);
        size
    }
}

#[no_mangle]
pub extern "C" fn get_blob_data(blob_ptr: *mut c_void) -> *const u8 {
    if blob_ptr.is_null() {
        return std::ptr::null();
    }
    unsafe {
        let blob: ID3DBlob = std::mem::transmute(blob_ptr);
        let data = blob.GetBufferPointer() as *const u8;
        std::mem::forget(blob);
        data
    }
}

#[no_mangle]
pub extern "C" fn free_blob(blob_ptr: *mut c_void) {
    if !blob_ptr.is_null() {
        unsafe {
            let _blob: ID3DBlob = std::mem::transmute(blob_ptr);
            // Blob will be released on drop
        }
    }
}