// shader.rs
use std::ffi::c_void;
use std::fs;
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
        ppCode: *mut Option<ID3DBlob>,
        ppErrorMsgs: *mut Option<ID3DBlob>,
    ) -> HRESULT;

    fn D3DCreateBlob(
        Size: usize,
        ppBlob: *mut Option<ID3DBlob>,
    ) -> HRESULT;
}

#[repr(C)]
struct D3D_SHADER_MACRO {
    name: PCSTR,
    Definition: PCSTR,
}

// Компиляция HLSL из файла
pub extern "C" fn compile_shader_from_file(
    file_path: *const i8,
    entry_point: *const i8,
    profile: *const i8,
    out_blob: *mut *mut c_void,
) -> bool {
    unsafe {
        let path_str = std::ffi::CStr::from_ptr(file_path).to_string_lossy();
        let entry = std::ffi::CStr::from_ptr(entry_point);
        let prof = std::ffi::CStr::from_ptr(profile);

        debug_println!("[compile_shader] Loading: {}", path_str);

        let hlsl_code = match fs::read_to_string(Path::new(path_str.as_ref())) {
            Ok(code) => code,
            Err(e) => {
                debug_println!("[compile_shader] Failed to read file: {}", e);
                return false;
            }
        };

        debug_println!("[compile_shader] Compiling {} ({}) - {} bytes",
                      entry.to_string_lossy(), prof.to_string_lossy(), hlsl_code.len());

        let mut blob: Option<ID3DBlob> = None;
        let mut error_blob: Option<ID3DBlob> = None;

        let source_name = std::ffi::CString::new(path_str.as_ref()).unwrap();

        let hr = D3DCompile(
            hlsl_code.as_ptr() as *const core::ffi::c_void,
            hlsl_code.len(),
            PCSTR(source_name.as_ptr() as *const u8),
            std::ptr::null(),
            std::ptr::null(),
            PCSTR(entry.as_ptr() as *const u8),
            PCSTR(prof.as_ptr() as *const u8),
            0,
            0,
            &mut blob,
            &mut error_blob,
        );

        if hr.is_err() {
            if let Some(err) = error_blob {
                let err_ptr = err.GetBufferPointer();
                let err_size = err.GetBufferSize();
                let err_msg = std::slice::from_raw_parts(err_ptr as *const u8, err_size);
                debug_println!("[compile_shader] COMPILATION ERROR:\n{}",
                              String::from_utf8_lossy(err_msg));
            }
            return false;
        }

        if let Some(b) = blob {
            // ВАЖНО: Сохраняем сырой указатель и НЕ ЗАБЫВАЕМ его!
            let raw_ptr = b.as_raw() as *mut c_void;
            *out_blob = raw_ptr;

            // НЕ ВЫЗЫВАЕМ std::mem::forget!
            // Blob должен остаться живым, его освободит вызывающий код

            let size = b.GetBufferSize();
            debug_println!("[compile_shader] ✅ Compiled successfully ({} bytes)", size);

            // Забываем Option, но сам объект продолжает жить через raw_ptr
            std::mem::forget(b);
            true
        } else {
            debug_println!("[compile_shader] No blob returned");
            false
        }
    }
}

// Для обратной совместимости
#[no_mangle]
pub extern "C" fn compile_hlsl(
    hlsl_code: *const i8,
    entry_point: *const i8,
    profile: *const i8,
    out_blob: *mut *mut c_void,
) -> bool {
    unsafe {
        let code = std::ffi::CStr::from_ptr(hlsl_code);
        let entry = std::ffi::CStr::from_ptr(entry_point);
        let prof = std::ffi::CStr::from_ptr(profile);

        let code_str = code.to_string_lossy();
        let entry_str = entry.to_string_lossy();
        let prof_str = prof.to_string_lossy();

        // Сохраняем во временный файл
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("shader_{}.hlsl", std::process::id()));
        if let Err(e) = fs::write(&temp_file, code_str.as_bytes()) {
            debug_println!("[compile_hlsl] Failed to write temp file: {}", e);
            return false;
        }

        let temp_path = temp_file.to_string_lossy();
        // ИСПРАВЛЕНО: CString из &str, а не из String
        let c_path = std::ffi::CString::new(temp_path.as_ref()).unwrap();
        let c_entry = std::ffi::CString::new(entry_str.as_ref()).unwrap();
        let c_prof = std::ffi::CString::new(prof_str.as_ref()).unwrap();

        let result = compile_shader_from_file(
            c_path.as_ptr(),
            c_entry.as_ptr(),
            c_prof.as_ptr(),
            out_blob,
        );

        let _ = fs::remove_file(temp_file);
        result
    }
}

#[no_mangle]
pub extern "C" fn load_precompiled_shader(
    data_ptr: *const u8,
    data_size: usize,
    out_blob: *mut *mut c_void,
) -> i32 {
    if data_ptr.is_null() || out_blob.is_null() || data_size == 0 {
        return -1;
    }

    unsafe {
        let mut blob: Option<ID3DBlob> = None;
        let hr = D3DCreateBlob(data_size, &mut blob);

        if hr.is_ok() {
            if let Some(b) = blob {
                let dst = b.GetBufferPointer();
                std::ptr::copy_nonoverlapping(data_ptr, dst as *mut u8, data_size);
                *out_blob = b.as_raw() as *mut c_void;
                std::mem::forget(b);
                return 0;
            }
        }
    }

    -1
}