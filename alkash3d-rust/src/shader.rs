// shader.rs - Исправленная версия с встроенными шейдерами
use std::ffi::c_void;
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
        ppCode: *mut Option<ID3DBlob>,
        ppErrorMsgs: *mut Option<ID3DBlob>,
    ) -> HRESULT;
}

#[repr(C)]
struct D3D_SHADER_MACRO {
    name: PCSTR,
    Definition: PCSTR,
}

// Встроенный вершинный шейдер
const BUILTIN_VS: &str = r#"
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

// Встроенный пиксельный шейдер
const BUILTIN_PS: &str = r#"
struct PSInput {
    float4 position : SV_POSITION;
    float4 color : COLOR;
};

float4 main(PSInput input) : SV_TARGET {
    return input.color;
}
"#;

#[no_mangle]
pub extern "C" fn get_builtin_vs_blob() -> *mut c_void {
    unsafe {
        debug_println!("[get_builtin_vs_blob] Compiling built-in VS...");

        let entry = std::ffi::CString::new("main").unwrap();
        let profile = std::ffi::CString::new("vs_5_0").unwrap();

        let mut blob: Option<ID3DBlob> = None;
        let mut error_blob: Option<ID3DBlob> = None;

        let hr = D3DCompile(
            BUILTIN_VS.as_ptr() as *const core::ffi::c_void,
            BUILTIN_VS.len(),
            PCSTR(b"builtin_vs.hlsl\0".as_ptr()),
            std::ptr::null(),
            std::ptr::null(),
            PCSTR(entry.as_ptr() as *const u8),
            PCSTR(profile.as_ptr() as *const u8),
            0,  // No debug
            0,
            &mut blob,
            &mut error_blob,
        );

        if hr.is_err() {
            if let Some(err) = error_blob {
                let err_ptr = err.GetBufferPointer();
                let err_size = err.GetBufferSize();
                let err_msg = std::slice::from_raw_parts(err_ptr as *const u8, err_size);
                debug_println!("[get_builtin_vs_blob] VS ERROR:\n{}", String::from_utf8_lossy(err_msg));
            }
            return std::ptr::null_mut();
        }

        if let Some(b) = blob {
            let raw_ptr = b.as_raw() as *mut c_void;
            std::mem::forget(b);
            debug_println!("[get_builtin_vs_blob] ✅ VS compiled");
            raw_ptr
        } else {
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn get_builtin_ps_blob() -> *mut c_void {
    unsafe {
        debug_println!("[get_builtin_ps_blob] Compiling built-in PS...");

        let entry = std::ffi::CString::new("main").unwrap();
        let profile = std::ffi::CString::new("ps_5_0").unwrap();

        let mut blob: Option<ID3DBlob> = None;
        let mut error_blob: Option<ID3DBlob> = None;

        let hr = D3DCompile(
            BUILTIN_PS.as_ptr() as *const core::ffi::c_void,
            BUILTIN_PS.len(),
            PCSTR(b"builtin_ps.hlsl\0".as_ptr()),
            std::ptr::null(),
            std::ptr::null(),
            PCSTR(entry.as_ptr() as *const u8),
            PCSTR(profile.as_ptr() as *const u8),
            0,  // No debug
            0,
            &mut blob,
            &mut error_blob,
        );

        if hr.is_err() {
            if let Some(err) = error_blob {
                let err_ptr = err.GetBufferPointer();
                let err_size = err.GetBufferSize();
                let err_msg = std::slice::from_raw_parts(err_ptr as *const u8, err_size);
                debug_println!("[get_builtin_ps_blob] PS ERROR:\n{}", String::from_utf8_lossy(err_msg));
            }
            return std::ptr::null_mut();
        }

        if let Some(b) = blob {
            let raw_ptr = b.as_raw() as *mut c_void;
            std::mem::forget(b);
            debug_println!("[get_builtin_ps_blob] ✅ PS compiled");
            raw_ptr
        } else {
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn free_blob(blob_ptr: *mut c_void) {
    if !blob_ptr.is_null() {
        unsafe {
            let _blob: ID3DBlob = std::mem::transmute_copy(&blob_ptr);
            // Blob will be dropped automatically
        }
    }
}