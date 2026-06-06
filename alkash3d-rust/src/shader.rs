// src/shader.rs - ИСПРАВЛЕННЫЙ ШЕЙДЕР

use std::ffi::c_void;
use std::ptr;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows_core::*;

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
    definition: PCSTR,
}

// ПРАВИЛЬНЫЙ VERTEX SHADER - читает из вершинного буфера
const VS_SOURCE: &str = r"
struct VSInput {
    float3 position : POSITION;
    float4 color : COLOR;
};

struct VSOutput {
    float4 position : SV_POSITION;
    float4 color : COLOR;
};

VSOutput main(VSInput input) {
    VSOutput output;
    output.position = float4(input.position, 1.0);
    output.color = input.color;
    return output;
}
";

// PIXEL SHADER
const PS_SOURCE: &str = r"
struct PSInput {
    float4 position : SV_POSITION;
    float4 color : COLOR;
};

float4 main(PSInput input) : SV_TARGET {
    return input.color;
}
";

// Глобальные данные
static mut VS_BLOB: *mut c_void = ptr::null_mut();
static mut PS_BLOB: *mut c_void = ptr::null_mut();
static mut VS_DATA: *const u8 = ptr::null();
static mut PS_DATA: *const u8 = ptr::null();
static mut VS_SIZE: usize = 0;
static mut PS_SIZE: usize = 0;

fn compile_shader(source: &str, target: &str, entry: &str) -> (*mut c_void, *const u8, usize) {
    let entry_cstr = std::ffi::CString::new(entry).unwrap();
    let target_cstr = std::ffi::CString::new(target).unwrap();

    let mut code_blob: *mut c_void = ptr::null_mut();
    let mut error_blob: *mut c_void = ptr::null_mut();

    unsafe {
        let hr = D3DCompile(
            source.as_ptr() as *const core::ffi::c_void,
            source.len(),
            PCSTR(b"shader.hlsl\0".as_ptr()),
            ptr::null(),
            ptr::null(),
            PCSTR(entry_cstr.as_ptr() as *const u8),
            PCSTR(target_cstr.as_ptr() as *const u8),
            0, 0,
            &mut code_blob,
            &mut error_blob,
        );

        if hr.is_err() {
            if !error_blob.is_null() {
                let err_blob = ID3DBlob::from_raw(error_blob as *mut _);
                let err_ptr = err_blob.GetBufferPointer();
                let err_size = err_blob.GetBufferSize();
                if err_size > 0 && !err_ptr.is_null() {
                    let err_msg = std::slice::from_raw_parts(err_ptr as *const u8, err_size);
                    eprintln!("Shader error:\n{}", String::from_utf8_lossy(err_msg));
                }
            }
            return (ptr::null_mut(), ptr::null(), 0);
        }

        if code_blob.is_null() {
            return (ptr::null_mut(), ptr::null(), 0);
        }

        let blob = ID3DBlob::from_raw(code_blob as *mut _);
        let data_ptr = blob.GetBufferPointer();
        let data_size = blob.GetBufferSize();

        let raw_ptr = blob.as_raw();
        std::mem::forget(blob);

        println!("[compile_shader] {} compiled, size={}", target, data_size);

        (raw_ptr as *mut c_void, data_ptr as *const u8, data_size)
    }
}

#[no_mangle]
pub extern "C" fn init_builtin_shaders() {
    unsafe {
        if VS_BLOB.is_null() {
            println!("[init_builtin_shaders] Compiling VS...");
            let (blob, data, size) = compile_shader(VS_SOURCE, "vs_5_0", "main");
            VS_BLOB = blob;
            VS_DATA = data;
            VS_SIZE = size;
        }
        if PS_BLOB.is_null() {
            println!("[init_builtin_shaders] Compiling PS...");
            let (blob, data, size) = compile_shader(PS_SOURCE, "ps_5_0", "main");
            PS_BLOB = blob;
            PS_DATA = data;
            PS_SIZE = size;
        }
    }
}

#[no_mangle]
pub extern "C" fn get_builtin_vs_blob() -> *mut c_void {
    unsafe {
        if VS_BLOB.is_null() {
            init_builtin_shaders();
        }
        VS_BLOB
    }
}

#[no_mangle]
pub extern "C" fn get_builtin_ps_blob() -> *mut c_void {
    unsafe {
        if PS_BLOB.is_null() {
            init_builtin_shaders();
        }
        PS_BLOB
    }
}

#[no_mangle]
pub extern "C" fn get_builtin_vs_data() -> *const u8 {
    unsafe {
        if VS_DATA.is_null() {
            init_builtin_shaders();
        }
        VS_DATA
    }
}

#[no_mangle]
pub extern "C" fn get_builtin_ps_data() -> *const u8 {
    unsafe {
        if PS_DATA.is_null() {
            init_builtin_shaders();
        }
        PS_DATA
    }
}

#[no_mangle]
pub extern "C" fn get_builtin_vs_size() -> usize {
    unsafe {
        if VS_SIZE == 0 {
            init_builtin_shaders();
        }
        VS_SIZE
    }
}

#[no_mangle]
pub extern "C" fn get_builtin_ps_size() -> usize {
    unsafe {
        if PS_SIZE == 0 {
            init_builtin_shaders();
        }
        PS_SIZE
    }
}

// Заглушки
#[no_mangle]
pub extern "C" fn get_test_vs_blob() -> *mut c_void { get_builtin_vs_blob() }
#[no_mangle]
pub extern "C" fn get_advanced_vs_blob() -> *mut c_void { get_builtin_vs_blob() }
#[no_mangle]
pub extern "C" fn get_advanced_ps_blob() -> *mut c_void { get_builtin_ps_blob() }
#[no_mangle]
pub extern "C" fn get_builtin_shader(_t: u32, is_vertex: bool) -> *mut c_void {
    if is_vertex { get_builtin_vs_blob() } else { get_builtin_ps_blob() }
}
#[no_mangle]
pub extern "C" fn destroy_shader_blob(_p: *mut c_void) -> bool { true }
#[no_mangle]
pub extern "C" fn free_blob(_p: *mut c_void) -> bool { true }
#[no_mangle]
pub extern "C" fn compile_shader_from_file(_a: *const i8, _b: *const i8, _c: *const i8) -> *mut c_void { ptr::null_mut() }
#[no_mangle]
pub extern "C" fn compile_shader_from_source(_a: *const i8, _b: *const i8, _c: *const i8) -> *mut c_void { ptr::null_mut() }
#[no_mangle]
pub extern "C" fn get_blob_size(p: *mut c_void) -> usize {
    if p.is_null() { return 0; }
    unsafe {
        let blob = &*(p as *const ID3DBlob);
        blob.GetBufferSize()
    }
}
#[no_mangle]
pub extern "C" fn get_blob_data(p: *mut c_void) -> *const u8 {
    if p.is_null() { return ptr::null(); }
    unsafe {
        let blob = &*(p as *const ID3DBlob);
        blob.GetBufferPointer() as *const u8
    }
}