//! ДОБАВЛЕНО (непрямой свет лучами, DDGI): компиляция HLSL через DXC —
//! Shader Model 6.x (inline ray tracing `RayQuery` требует cs_6_5+), чего
//! старый FXC (`shader.rs`, D3DCompile, до SM 5.1) не умеет.
//!
//! Библиотеки DXC не входят в Windows: `dxcompiler.dll` (компилятор) и
//! `dxil.dll` (подпись DXIL — без неё D3D12 не примет шейдер) ищутся в
//! каталоге из `ALKASH3D_DXC_DIR`, рядом с exe, затем в Windows SDK
//! (`Redist/D3D/x64`). Обе грузятся по полному пути один раз (компилятор
//! ищет "dxil.dll" по имени и находит уже загруженный модуль), а
//! `DxcCreateInstance` берётся через GetProcAddress: обёртка windows-rs
//! вписала бы dxcompiler.dll в импорт exe, и без DXC не запустился бы
//! вообще ни один бинарник движка.

use std::path::PathBuf;
use std::sync::OnceLock;
use windows::core::*;
use windows::Win32::Graphics::Direct3D::Dxc::*;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::shader::ShaderBlob;

/// Каталоги, где искать DXC, по порядку.
fn candidates() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(d) = std::env::var("ALKASH3D_DXC_DIR") {
        dirs.push(PathBuf::from(d));
    }
    if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        dirs.push(d);
    }
    let kits = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10");
    dirs.push(kits.join(r"Redist\D3D\x64"));
    // Самые новые SDK — первыми.
    if let Ok(entries) = std::fs::read_dir(kits.join("bin")) {
        let mut versions: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path().join("x64")).collect();
        versions.sort();
        dirs.extend(versions.into_iter().rev());
    }
    dirs
}

type CreateInstance = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut core::ffi::c_void) -> HRESULT;

/// Загрузить dxil.dll и dxcompiler.dll; адрес `DxcCreateInstance` или где искали.
fn load() -> std::result::Result<CreateInstance, String> {
    static LOADED: OnceLock<std::result::Result<usize, String>> = OnceLock::new();
    LOADED.get_or_init(load_once).clone().map(|f| unsafe { std::mem::transmute::<usize, CreateInstance>(f) })
}

fn load_once() -> std::result::Result<usize, String> {
    let dirs = candidates();
    for dir in &dirs {
        let (dxil, dxc) = (dir.join("dxil.dll"), dir.join("dxcompiler.dll"));
        if !dxil.exists() || !dxc.exists() {
            continue;
        }
        let wide = |p: &PathBuf| -> Vec<u16> { p.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect() };
        unsafe {
            let Ok(_) = LoadLibraryW(PCWSTR(wide(&dxil).as_ptr())) else { continue };
            let Ok(module) = LoadLibraryW(PCWSTR(wide(&dxc).as_ptr())) else { continue };
            if let Some(f) = GetProcAddress(module, s!("DxcCreateInstance")) {
                println!("[DXC] ✓ dxcompiler.dll + dxil.dll из {}", dir.display());
                return Ok(f as usize);
            }
        }
    }
    Err(format!("DXC не найден (dxcompiler.dll + dxil.dll): {:?}", dirs))
}

/// `DxcCreateInstance` для интерфейса `T`.
unsafe fn create<T: Interface>(create: CreateInstance, clsid: &GUID) -> Result<T> {
    let mut p = std::ptr::null_mut();
    unsafe {
        create(clsid, &T::IID, &mut p).ok()?;
        Ok(T::from_raw(p))
    }
}

/// Есть ли DXC на этой машине.
pub fn available() -> bool {
    load().is_ok()
}

/// Скомпилировать `source` (`target` — например "cs_6_5", `entry` — точка
/// входа). Ошибки компилятора печатаются и возвращаются как E_FAIL.
pub fn compile(source: &str, target: &str, entry: &str) -> Result<ShaderBlob> {
    let create_instance = match load() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[DXC] {}", e);
            return Err(Error::from_hresult(HRESULT(0x80004005u32 as i32)));
        }
    };
    println!("[DXC] Compiling {} - entry: {}", target, entry);
    // Старый интерфейс IDxcCompiler: его понимают все версии DXC, в том числе
    // из Windows SDK 19041 (IDxcCompiler3 там ещё нет), а SM 6.5 и RayQuery
    // эта версия уже компилирует.
    unsafe {
        let library: IDxcLibrary = create(create_instance, &CLSID_DxcLibrary)?;
        let compiler: IDxcCompiler = create(create_instance, &CLSID_DxcCompiler)?;
        let blob = library.CreateBlobWithEncodingFromPinned(source.as_ptr() as *const _, source.len() as u32, DXC_CP_UTF8)?;
        let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() };
        let (name, e, t, o3) = (wide("shader.hlsl"), wide(entry), wide(target), wide("-O3"));
        let args = [PCWSTR(o3.as_ptr())];
        let result = compiler.Compile(&blob, PCWSTR(name.as_ptr()), PCWSTR(e.as_ptr()), PCWSTR(t.as_ptr()), Some(&args), &[], None::<&IDxcIncludeHandler>)?;

        let message = result
            .GetErrorBuffer()
            .ok()
            .filter(|b| b.GetBufferSize() > 0)
            .map(|b| String::from_utf8_lossy(std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize())).trim_end_matches('\0').to_string())
            .filter(|m| !m.trim().is_empty());
        if result.GetStatus()?.is_err() {
            eprintln!("[DXC] Shader compilation error:
{}", message.unwrap_or_default());
            return Err(Error::from_hresult(HRESULT(0x80004005u32 as i32)));
        }
        if let Some(m) = message {
            println!("[DXC] предупреждения:
{}", m);
        }
        let object = result.GetResult()?;
        let data = std::slice::from_raw_parts(object.GetBufferPointer() as *const u8, object.GetBufferSize()).to_vec();
        println!("[DXC] ✓ Compiled successfully, {} bytes", data.len());
        Ok(ShaderBlob { data })
    }
}
