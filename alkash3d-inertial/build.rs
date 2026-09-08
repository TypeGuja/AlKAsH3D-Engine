// inertial/build.rs
//! Собирает Fortran-ядра (.f90) через gfortran и линкует их в inertial.dll.
//!
//! ТРЕБОВАНИЯ ДЛЯ СБОРКИ:
//!  - Установленный gfortran (GCC Fortran) в PATH.
//!  - На Windows: НУЖЕН MinGW-w64 тулчейн (например, через MSYS2 —
//!    `pacman -S mingw-w64-x86_64-gcc-fortran mingw-w64-x86_64-toolchain`).
//!    Стандартный MSVC-тулчейн Fortran не собирает — gfortran тянет свои
//!    libgcc/libgfortran/libgomp рантаймы, которые понимает только связка
//!    GNU-компилятор + GNU-линкер. Поэтому сам Rust-крейт "inertial" тоже
//!    нужно собирать под GNU-таргет — см. .cargo/config.toml в этом же
//!    крейте, он уже фиксирует x86_64-pc-windows-gnu по умолчанию.
//!  - На Linux/macOS gfortran из системного пакетного менеджера подходит
//!    без всяких оговорок (apt install gfortran / brew install gcc).
//!
//! ЧТО ДЕЛАЕТ:
//!  1. Компилирует каждый .f90 в .o с флагами -O3 -fopenmp -fPIC.
//!     ПОРЯДОК ВАЖЕН: rigid_body.f90 объявляет модуль (rigid_body_mod),
//!     который импортируют все остальные файлы — он должен быть
//!     скомпилирован ПЕРВЫМ, иначе gfortran не найдёт файл интерфейса
//!     модуля (.mod).
//!  2. Собирает все .o в статическую библиотеку libinertial_kernels.a.
//!  3. Говорит cargo слинковать эту библиотеку + рантаймы libgfortran/libgomp.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR не задан cargo'м"));
    let kernels_dir = PathBuf::from("src/kernels");

    // Порядок важен для модульных зависимостей Fortran — см. комментарий выше.
    // ПРИМЕЧАНИЕ: reference.f90 больше не существует — integrate_bodies/
    // solve_contacts/update_aabb УЖЕ реализованы в rigid_body.f90, отдельный
    // файл с теми же bind(c)-именами только создавал бы риск конфликта
    // символов при линковке.
    let sources = [
        "rigid_body.f90",
        "broad_phase.f90",
        "narrow_phase.f90",
        "solver.f90",
        "kernels_optimized.f90",
    ];

    let mut object_files = Vec::new();

    for src in sources {
        let src_path = kernels_dir.join(src);
        println!("cargo:rerun-if-changed={}", src_path.display());

        if !src_path.exists() {
            panic!(
                "Не найден исходник Fortran: {}. Ожидается, что все .f90 \
                 лежат в {}/",
                src_path.display(),
                kernels_dir.display()
            );
        }

        let obj_path = out_dir.join(format!("{}.o", src.trim_end_matches(".f90")));

        let status = Command::new("gfortran")
            .arg("-c")
            .arg("-O3")
            .arg("-fopenmp")
            .arg("-fPIC")
            .arg("-J").arg(&out_dir)
            .arg("-I").arg(&out_dir)
            .arg(&src_path)
            .arg("-o").arg(&obj_path)
            .status()
            .unwrap_or_else(|e| {
                panic!(
                    "Не удалось запустить gfortran для {}: {}.\n\
                     Убедитесь, что gfortran установлен и есть в PATH \
                     (Linux/macOS: apt/brew install gfortran; \
                     Windows: MSYS2 mingw-w64-x86_64-gcc-fortran).",
                    src, e
                )
            });

        if !status.success() {
            panic!("gfortran не смог скомпилировать {} (см. вывод выше)", src);
        }

        object_files.push(obj_path);
    }

    let lib_path = out_dir.join("libinertial_kernels.a");
    let mut ar_cmd = Command::new("ar");
    ar_cmd.arg("crs").arg(&lib_path);
    for obj in &object_files {
        ar_cmd.arg(obj);
    }
    let status = ar_cmd.status().unwrap_or_else(|e| {
        panic!(
            "Не удалось запустить ar: {}. Убедитесь, что binutils установлены \
             (обычно ставятся вместе с gcc/gfortran).",
            e
        )
    });
    if !status.success() {
        panic!("ar не смог собрать libinertial_kernels.a из объектных файлов");
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=inertial_kernels");

    println!("cargo:rustc-link-lib=dylib=gfortran");
    println!("cargo:rustc-link-lib=dylib=gomp");

    // ДОБАВЛЕНО (диагностика "LoadLibraryExW failed" на реальной машине
    // пользователя): строки выше линкуют `inertial.dll` ДИНАМИЧЕСКИ с
    // рантаймами gfortran/OpenMP (`libgfortran-5.dll`/`libgomp-1.dll`, и
    // транзитивно `libquadmath-0.dll`/`libwinpthread-1.dll`/
    // `libgcc_s_seh-1.dll`) — все они живут только в MSYS2
    // (`<mingw64>\bin`) и НЕ копируются рядом с итоговой `inertial.dll`
    // сами по себе. Раньше это "работало" только пока эта папка была в
    // PATH процесса, который грузит плагин (см. подробности в
    // `alkash3d-rust/src/plugin/manager.rs::load_library` — там же теперь
    // явно запрошен поиск зависимостей ещё и В ПАПКЕ САМОЙ DLL). Обе
    // правки нужны вместе: копия DLL рядом бесполезна без флага поиска на
    // стороне загрузчика, а флаг поиска бесполезен, если копий тут нет.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        copy_runtime_dlls(&out_dir);
    }
}

/// Копирует рантайм-DLL gfortran/OpenMP рядом с итоговой `inertial.dll` —
/// и в `target/<triple>/<profile>/`, и в `.../deps/` (примеры вида
/// `cargo run --example joint_test` собираются и запускаются именно из
/// `deps/`, им отдельно нужны те же DLL рядом). Папку с DLL ищем через
/// `where gfortran` — тот же PATH-контракт, что этот build.rs УЖЕ требует
/// для самой компиляции Fortran-ядер несколькими строками выше, новой
/// зависимости не добавляет.
fn copy_runtime_dlls(out_dir: &PathBuf) {
    const RUNTIME_DLLS: [&str; 5] = [
        "libgfortran-5.dll",
        "libgomp-1.dll",
        "libquadmath-0.dll",
        "libwinpthread-1.dll",
        "libgcc_s_seh-1.dll",
    ];

    let where_output = Command::new("where").arg("gfortran").output();
    let gfortran_dir = match where_output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.lines().next().and_then(|line| {
                PathBuf::from(line.trim()).parent().map(|p| p.to_path_buf())
            })
        }
        _ => None,
    };

    let Some(gfortran_dir) = gfortran_dir else {
        println!(
            "cargo:warning=inertial: не удалось найти папку gfortran через `where gfortran` \
             — рантайм-DLL (libgfortran/libgomp/...) не скопированы рядом с inertial.dll. \
             Плагин загрузится, только если эта папка (обычно <MSYS2>\\mingw64\\bin) есть в PATH \
             процесса, который его грузит."
        );
        return;
    };

    // out_dir = target/<triple>/<profile>/build/inertial-<hash>/out
    // -> подняться на 3 уровня до target/<triple>/<profile>/
    let Some(profile_dir) = out_dir.parent().and_then(|p| p.parent()).and_then(|p| p.parent()) else {
        return;
    };
    let deps_dir = profile_dir.join("deps");

    for dll in RUNTIME_DLLS {
        let src = gfortran_dir.join(dll);
        if !src.exists() {
            println!("cargo:warning=inertial: {} не найден в {}", dll, gfortran_dir.display());
            continue;
        }
        let _ = std::fs::copy(&src, profile_dir.join(dll));
        let _ = std::fs::create_dir_all(&deps_dir);
        let _ = std::fs::copy(&src, deps_dir.join(dll));
    }
}
