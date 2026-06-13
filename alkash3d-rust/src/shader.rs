// src/shader.rs - полная версия с компиляцией HLSL
use windows::core::*;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use std::ffi::CString;
use windows::Win32::Graphics::Direct3D::Fxc::*;

#[derive(Clone)]
pub struct ShaderBlob {
    pub data: Vec<u8>,
}

impl ShaderBlob {
    pub fn compile(source: &str, target: &str, entry_point: &str) -> Result<Self> {
        println!("[SHADER] Compiling {} - entry: {}", target, entry_point);

        unsafe {
            let mut blob: Option<ID3DBlob> = None;
            let mut error_blob: Option<ID3DBlob> = None;

            let source_bytes = source.as_bytes();
            let source_ptr = source_bytes.as_ptr() as *const _;
            let source_len = source_bytes.len();

            let entry_cstr = CString::new(entry_point).unwrap();
            let target_cstr = CString::new(target).unwrap();

            let hr = D3DCompile(
                source_ptr,
                source_len,
                None,
                None,
                None,
                PCSTR(entry_cstr.as_ptr() as *const u8),
                PCSTR(target_cstr.as_ptr() as *const u8),
                D3DCOMPILE_OPTIMIZATION_LEVEL3,
                0,
                &mut blob,
                Some(&mut error_blob),
            );

            if hr.is_err() {
                if let Some(error_blob) = error_blob {
                    let error = std::slice::from_raw_parts(
                        error_blob.GetBufferPointer() as *const u8,
                        error_blob.GetBufferSize(),
                    );
                    let error_str = String::from_utf8_lossy(error);
                    eprintln!("Shader compilation error:\n{}", error_str);
                }
                return Err(Error::from_hresult(HRESULT::from(hr)));
            }

            let blob = blob.unwrap();
            let data = std::slice::from_raw_parts(
                blob.GetBufferPointer() as *const u8,
                blob.GetBufferSize(),
            ).to_vec();

            println!("[SHADER] ✓ Compiled successfully, {} bytes", data.len());
            Ok(Self { data })
        }
    }

    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self { data }
    }

    pub fn as_ptr(&self) -> *const std::ffi::c_void {
        self.data.as_ptr() as *const _
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }
}