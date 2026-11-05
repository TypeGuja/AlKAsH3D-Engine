//! Загрузка шага деформируемого тела на Fortran из inertial.dll (там же ядра твёрдых
//! тел). Сама `softbody` от DLL не зависит: без неё считает эталон на Rust.

use crate::softbody::StepKernel;
use libloading::Library;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::OnceLock;

const RELATIVE: &str = "../alkash3d-inertial/target/x86_64-pc-windows-gnu/release/inertial.dll";

fn candidates() -> Vec<PathBuf> {
    let mut tries = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.to_path_buf())) {
        tries.push(dir.join("inertial.dll"));
    }
    tries.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(RELATIVE));
    tries
}

fn load() -> Result<StepKernel, String> {
    let path = candidates().into_iter().find(|p| p.exists()).ok_or("inertial.dll не найдена")?;
    // рантаймы Fortran лежат рядом с DLL — поиск от её папки, как у плагинов
    let library = crate::plugin::load_library(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let pointer = unsafe {
        let getter = library
            .get::<unsafe extern "C" fn() -> *const c_void>(b"inertial_softbody_step")
            .map_err(|e| format!("{}: старая сборка без ядра деформируемых тел: {}", path.display(), e))?;
        getter()
    };
    // библиотека нужна до конца работы программы
    std::mem::forget::<Library>(library);
    // SAFETY: inertial_softbody_step отдаёт адрес softbody_step с этой сигнатурой
    Ok(unsafe { std::mem::transmute::<*const c_void, StepKernel>(pointer) })
}

/// Ядро на Fortran; загружается один раз. Err — почему считать придётся на Rust.
pub fn step_kernel() -> Result<StepKernel, String> {
    static KERNEL: OnceLock<Result<StepKernel, String>> = OnceLock::new();
    KERNEL.get_or_init(load).clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::softbody_car::{build_car, simulate_with, Target};

    /// Ядро обязано считать то же, что эталон на Rust.
    #[test]
    #[ignore = "долго без оптимизации: cargo test --lib -- --ignored"]
    fn fortran_kernel_matches_the_rust_reference() {
        let kernel = match step_kernel() {
            Ok(k) => k,
            Err(e) => panic!("ядро на Fortran не загрузилось: {}", e),
        };
        // шаг в шаг на начале удара
        let mut a = build_car(5e-5);
        let mut b = build_car(5e-5);
        b.body.kernel = Some(kernel);
        for s in [&mut a, &mut b] {
            for v in s.body.vel.iter_mut() {
                v.x += 15.0;
            }
            s.body.obstacles.push(crate::softbody::Obstacle::Block { min: crate::softbody::v3(4.3, -1.0, -6.0), max: crate::softbody::v3(5.3, 4.0, 6.0) });
        }
        for n in 0..6000 {
            a.body.step();
            b.body.step();
            if n % 500 == 0 {
                let worst = a.body.pos.iter().zip(&b.body.pos).map(|(p, q)| (*p - *q).len()).fold(0.0, f64::max);
                assert!(worst < 1e-6, "шаг {}: разошлись на {} м", n, worst);
            }
        }
        // итоги всего удара
        for target in [Target::Wall, Target::Pole { offset: 0.0 }] {
            let r = simulate_with(None, target, 56.0, 0.3, 1000.0, |_| {}).report;
            let f = simulate_with(Some(kernel), target, 56.0, 0.3, 1000.0, |_| {}).report;
            assert!((r.dynamic_crush - f.dynamic_crush).abs() < 0.005, "{:?}: смятие {} и {}", target, r.dynamic_crush, f.dynamic_crush);
            assert!((r.plastic - f.plastic).abs() < r.plastic * 0.01, "{:?}: течение {} и {}", target, r.plastic, f.plastic);
        }
    }
}
