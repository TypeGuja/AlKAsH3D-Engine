//! Stereo VR smoke scene using the engine's OpenXR runtime.
//! `cargo run --features xr --bin xr_demo`
//! `ALKASH3D_XR_MODE=ar` selects real-world composition when available.

use alkash3d_rs::engine::MeshInstance;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--mock") {
        return alkash3d_rs::xr::runtime::run_mock_copy();
    }
    let ar = std::env::var("ALKASH3D_XR_MODE").is_ok_and(|mode| mode.eq_ignore_ascii_case("ar"));
    alkash3d_rs::xr::runtime::run_xr(ar, |engine| {
        // Deliberately simple, high-contrast geometry.  It makes it obvious
        // in a headset that both eyes are rendering and positional tracking
        // changes the view: a floor, two side markers and a central cube.
        let floor = engine.add_cube_colored(1.0, 0.10, 0.13, 0.19, 1.0);
        engine.mesh_instances.push(
            MeshInstance::new(floor)
                .at(0.0, -1.1, -2.5)
                .scaled(6.0, 0.08, 6.0),
        );

        let center = engine.add_cube_colored(0.70, 0.12, 0.85, 1.0, 1.0);
        engine
            .mesh_instances
            .push(MeshInstance::new(center).at(0.0, 0.0, -2.0));

        let left = engine.add_cube_colored(0.35, 1.0, 0.22, 0.18, 1.0);
        engine
            .mesh_instances
            .push(MeshInstance::new(left).at(-1.25, 0.10, -2.4));

        let right = engine.add_cube_colored(0.35, 0.20, 1.0, 0.35, 1.0);
        engine
            .mesh_instances
            .push(MeshInstance::new(right).at(1.25, 0.10, -2.4));
    })
}
