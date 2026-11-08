//! Minimal scene using the engine's OpenXR runtime.
//! `cargo run --features xr --bin xr_demo`
//! `ALKASH3D_XR_MODE=ar` selects real-world composition when available.

use alkash3d_rs::engine::MeshInstance;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--mock") {
        return alkash3d_rs::xr::runtime::run_mock_copy();
    }
    let ar = std::env::var("ALKASH3D_XR_MODE")
        .is_ok_and(|mode| mode.eq_ignore_ascii_case("ar"));
    alkash3d_rs::xr::runtime::run_xr(ar, |engine| {
        let cube = engine.add_cube_colored(0.7, 0.2, 0.8, 0.8, 1.0);
        engine.mesh_instances.push(MeshInstance::new(cube).at(0.0, 0.0, -2.0));
    })
}
