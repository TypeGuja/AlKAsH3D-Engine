//! Reports capabilities of the active Windows OpenXR runtime without a headset frame loop.
//! Run with `cargo run --features xr --bin xr_probe`.

use openxr as xr;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let entry = alkash3d_rs::xr::runtime::load_openxr_entry()?;
    let available = entry.enumerate_extensions()?;
    println!("D3D12: {}", available.khr_d3d12_enable);
    println!("Passthrough (FB): {}", available.fb_passthrough);
    println!(
        "Spatial planes (EXT): {}",
        available.ext_spatial_plane_tracking
    );
    println!("Hand tracking (EXT): {}", available.ext_hand_tracking);
    if !available.khr_d3d12_enable {
        return Err("active OpenXR runtime does not support XR_KHR_D3D12_enable".into());
    }

    let mut enabled = xr::ExtensionSet::default();
    enabled.khr_d3d12_enable = true;
    let instance = entry.create_instance(
        &xr::ApplicationInfo {
            application_name: "AlKAsH3D XR probe",
            application_version: 1,
            engine_name: "AlKAsH3D",
            engine_version: 1,
            ..Default::default()
        },
        &enabled,
        &[],
        &(),
    ).map_err(|err| format!(
        "OpenXR runtime could not start ({err}). Start SteamVR (or the headset vendor app), connect the headset, then retry."
    ))?;
    let runtime = instance.properties()?;
    println!(
        "Runtime: {} {}",
        runtime.runtime_name, runtime.runtime_version
    );
    let system = instance
        .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
        .map_err(|err| {
            format!(
                "No VR headset is available ({err}). Make sure SteamVR shows the headset as ready."
            )
        })?;
    let properties = instance.system_properties(system)?;
    println!("System: {}", properties.system_name);
    println!(
        "Position tracking: {}",
        properties.tracking_properties.position_tracking
    );
    println!(
        "Orientation tracking: {}",
        properties.tracking_properties.orientation_tracking
    );
    let view_type = xr::ViewConfigurationType::PRIMARY_STEREO;
    for (index, view) in instance
        .enumerate_view_configuration_views(system, view_type)?
        .iter()
        .enumerate()
    {
        println!(
            "View {index}: {}x{}, {} samples recommended",
            view.recommended_image_rect_width,
            view.recommended_image_rect_height,
            view.recommended_swapchain_sample_count
        );
    }
    println!(
        "Blend modes: {:?}",
        instance.enumerate_environment_blend_modes(system, view_type)?
    );
    let requirements = instance.graphics_requirements::<xr::D3D12>(system)?;
    println!(
        "D3D12 adapter LUID: {:08X}:{:08X}",
        requirements.adapter_luid.HighPart as u32, requirements.adapter_luid.LowPart
    );
    println!(
        "D3D12 minimum feature level: {:?}",
        requirements.min_feature_level
    );
    Ok(())
}
