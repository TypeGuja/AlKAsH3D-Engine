//! Engine-owned OpenXR session and stereo frame loop for D3D12.

use std::{error::Error, ffi::c_void, time::Duration};
use crate::{
    engine::{AlkashEngine, GraphicsSettings, MeshInstance},
    math::{Quat, Vec3},
    xr::{XrFov, XrPose},
};
use openxr as xr;
use windows::{core::Interface, Win32::{Foundation::LUID, Graphics::{Direct3D::D3D_FEATURE_LEVEL, Direct3D12::*, Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, DXGI_SAMPLE_DESC}}}};

const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;

struct Eye {
    swapchain: xr::Swapchain<xr::D3D12>,
    images: Vec<*mut xr::sys::platform::ID3D12Resource>,
}

/// Starts a Windows XR session and calls `setup` after engine initialization.
/// `ar` selects real-world composition when supported by the active runtime.
pub fn run_xr<F>(ar: bool, setup: F) -> Result<(), Box<dyn Error>>
where
    F: FnOnce(&mut AlkashEngine),
{
    // SAFETY: the installed OpenXR loader implements the platform loader ABI.
    let entry = unsafe { xr::Entry::load(&()) }
        .map_err(|err| format!("OpenXR loader unavailable ({err}); install and activate your headset's OpenXR runtime"))?;
    let available = entry.enumerate_extensions()?;
    if !available.khr_d3d12_enable {
        return Err("OpenXR runtime has no D3D12 support".into());
    }
    let mut enabled = xr::ExtensionSet::default();
    enabled.khr_d3d12_enable = true;
    enabled.fb_passthrough = ar && available.fb_passthrough;
    let instance = entry.create_instance(
        &xr::ApplicationInfo {
            application_name: "AlKAsH3D XR demo",
            application_version: 1,
            engine_name: "AlKAsH3D",
            engine_version: 1,
            ..Default::default()
        },
        &enabled,
        &[],
        &(),
    )?;
    let system = instance.system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)?;
    let views = instance.enumerate_view_configuration_views(system, VIEW_TYPE)?;
    if views.len() != 2 || views[0].recommended_image_rect_width != views[1].recommended_image_rect_width
        || views[0].recommended_image_rect_height != views[1].recommended_image_rect_height
    {
        return Err("this demo requires two equally sized stereo views".into());
    }
    let (width, height) = (views[0].recommended_image_rect_width, views[0].recommended_image_rect_height);
    let blend_modes = instance.enumerate_environment_blend_modes(system, VIEW_TYPE)?;
    let blend = if ar {
        blend_modes.iter().copied().find(|m| *m == xr::EnvironmentBlendMode::ALPHA_BLEND)
            .or_else(|| blend_modes.iter().copied().find(|m| *m == xr::EnvironmentBlendMode::ADDITIVE))
            .or_else(|| if enabled.fb_passthrough {
                blend_modes.iter().copied().find(|m| *m == xr::EnvironmentBlendMode::OPAQUE)
            } else { None })
            .ok_or("the active OpenXR runtime offers no AR environment blend mode")?
    } else {
        blend_modes.iter().copied().find(|m| *m == xr::EnvironmentBlendMode::OPAQUE)
            .ok_or("the active OpenXR runtime offers no opaque VR blend mode")?
    };
    let use_fb_passthrough = ar && blend == xr::EnvironmentBlendMode::OPAQUE;
    let requirements = instance.graphics_requirements::<xr::D3D12>(system)?;
    let luid = LUID { LowPart: requirements.adapter_luid.LowPart, HighPart: requirements.adapter_luid.HighPart };
    let mut engine = AlkashEngine::new(width, height);
    engine.set_graphics_settings(GraphicsSettings {
        msaa: false, ssao: false, bloom: false, volumetric: false,
        shadows: false, render_scale: 1.0,
    });
    engine.init_for_xr(luid, D3D_FEATURE_LEVEL(requirements.min_feature_level))?;
    engine.process_messages();
    let renderer_size = engine.renderer.as_ref().map(|renderer| (renderer.width, renderer.height));
    if renderer_size != Some((width, height)) {
        return Err(format!("XR mirror size mismatch: expected {width}x{height}, got {renderer_size:?}").into());
    }
    engine.set_xr_alpha_blend(blend == xr::EnvironmentBlendMode::ALPHA_BLEND || use_fb_passthrough);
    engine.set_clear_color(0.0, 0.0, 0.0, if ar { 0.0 } else { 1.0 });
    setup(&mut engine);

    let device = crate::get_device()?;
    let queue = crate::get_command_queue()?;
    let graphics = xr::d3d::SessionCreateInfoD3D12 {
        device: device.as_raw() as _,
        queue: queue.as_raw() as _,
    };
    // SAFETY: the engine device uses the runtime's LUID and minimum feature
    // level; both COM objects remain alive until the session is dropped.
    let (session, mut frame_waiter, mut frame_stream) = unsafe {
        instance.create_session::<xr::D3D12>(system, &graphics)?
    };
    let space = session.create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)?;
    // The creation flag starts both objects without calling Passthrough::start,
    // which is implemented incorrectly in openxr 0.22 (it calls pause).
    let passthrough = if use_fb_passthrough {
        Some(session.create_passthrough(xr::PassthroughFlagsFB::IS_RUNNING_AT_CREATION)?)
    } else { None };
    let passthrough_layer = if let Some(passthrough) = &passthrough {
        Some(session.create_passthrough_layer(
            passthrough,
            xr::PassthroughFlagsFB::IS_RUNNING_AT_CREATION,
            xr::PassthroughLayerPurposeFB::RECONSTRUCTION,
        )?)
    } else { None };
    let formats = session.enumerate_swapchain_formats()?;
    let format = [DXGI_FORMAT_R8G8B8A8_UNORM_SRGB.0 as u32, DXGI_FORMAT_R8G8B8A8_UNORM.0 as u32]
        .into_iter().find(|format| formats.contains(format))
        .ok_or("OpenXR runtime does not accept an R8G8B8A8 color format")?;
    let mut eyes = Vec::with_capacity(2);
    for _ in 0..2 {
        let swapchain = session.create_swapchain(&xr::SwapchainCreateInfo {
            create_flags: xr::SwapchainCreateFlags::EMPTY,
            usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT | xr::SwapchainUsageFlags::TRANSFER_DST,
            format,
            sample_count: 1,
            width,
            height,
            face_count: 1,
            array_size: 1,
            mip_count: 1,
        })?;
        let images = swapchain.enumerate_images()?;
        eyes.push(Eye { swapchain, images });
    }
    println!("OpenXR {:?}: {}x{} per eye, {:?} blend", instance.properties()?.runtime_name, width, height, blend);

    let outcome = run(&instance, &session, &space, passthrough_layer.as_ref(), &mut frame_waiter, &mut frame_stream, &mut eyes, &mut engine, blend, width, height);
    engine.set_xr_output_target(None)?;
    engine.camera.clear_xr_matrices();
    engine.shutdown();
    outcome
}

/// Checks the same D3D12 copy path with a local texture when no XR runtime
/// or headset is installed. It does not simulate tracking or composition.
pub fn run_mock_copy() -> Result<(), Box<dyn Error>> {
    let (width, height) = (640, 480);
    let mut engine = AlkashEngine::new(width, height);
    engine.set_graphics_settings(GraphicsSettings {
        msaa: false, ssao: false, bloom: false, volumetric: false,
        shadows: false, render_scale: 1.0,
    });
    engine.init()?;
    engine.process_messages();
    let (actual_width, actual_height) = engine.renderer.as_ref()
        .map(|renderer| (renderer.width, renderer.height))
        .ok_or("renderer unavailable after initialization")?;
    let cube = engine.add_cube_colored(0.7, 0.2, 0.8, 0.8, 1.0);
    engine.mesh_instances.push(MeshInstance::new(cube).at(0.0, 0.0, 0.0));
    let device = crate::get_device()?;
    let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Alignment: 0,
        Width: actual_width as u64,
        Height: actual_height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
        Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
    };
    let mut texture: Option<ID3D12Resource> = None;
    unsafe {
        device.CreateCommittedResource(
            &heap, D3D12_HEAP_FLAG_NONE, &desc,
            D3D12_RESOURCE_STATE_RENDER_TARGET, None, &mut texture,
        )?;
    }
    let texture = texture.ok_or("D3D12 did not create the mock XR texture")?;
    // Keep the texture alive until shutdown waits for the GPU copy to finish.
    engine.set_xr_output_target(Some(texture.clone()))?;
    let result = engine.render_frame();
    engine.set_xr_output_target(None)?;
    engine.shutdown();
    result?;
    println!("XR copy smoke test passed ({}x{})", desc.Width, desc.Height);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run(
    instance: &xr::Instance,
    session: &xr::Session<xr::D3D12>,
    space: &xr::Space,
    passthrough_layer: Option<&xr::PassthroughLayerFB>,
    frame_waiter: &mut xr::FrameWaiter,
    frame_stream: &mut xr::FrameStream<xr::D3D12>,
    eyes: &mut [Eye],
    engine: &mut AlkashEngine,
    blend: xr::EnvironmentBlendMode,
    width: u32,
    height: u32,
) -> Result<(), Box<dyn Error>> {
    let mut events = xr::EventDataBuffer::new();
    let mut running = false;
    loop {
        engine.process_messages();
        if !engine.is_running() {
            if running { session.request_exit()?; }
            break;
        }
        while let Some(event) = instance.poll_event(&mut events)? {
            match event {
                xr::Event::SessionStateChanged(change) => match change.state() {
                    xr::SessionState::READY => { session.begin(VIEW_TYPE)?; running = true; }
                    xr::SessionState::STOPPING => { session.end()?; running = false; }
                    xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => return Ok(()),
                    _ => {}
                },
                xr::Event::InstanceLossPending(_) => return Ok(()),
                _ => {}
            }
        }
        if !running {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        let frame = frame_waiter.wait()?;
        frame_stream.begin()?;
        if !frame.should_render {
            frame_stream.end(frame.predicted_display_time, blend, &[])?;
            continue;
        }
        let (flags, views) = session.locate_views(VIEW_TYPE, frame.predicted_display_time, space)?;
        if views.len() != 2 || !flags.contains(xr::ViewStateFlags::POSITION_VALID | xr::ViewStateFlags::ORIENTATION_VALID) {
            frame_stream.end(frame.predicted_display_time, blend, &[])?;
            continue;
        }
        let rect = xr::Rect2Di {
            offset: xr::Offset2Di { x: 0, y: 0 },
            extent: xr::Extent2Di { width: width as _, height: height as _ },
        };
        for (eye, view) in eyes.iter_mut().zip(&views) {
            let index = eye.swapchain.acquire_image()? as usize;
            eye.swapchain.wait_image(xr::Duration::INFINITE)?;
            let raw = eye.images[index] as *mut c_void;
            // SAFETY: the acquired image stays owned by this swapchain until
            // release_image; clone obtains a COM reference for the renderer.
            let resource: ID3D12Resource = unsafe { ID3D12Resource::from_raw_borrowed(&raw) }
                .ok_or("OpenXR returned a null D3D12 swapchain image")?.clone();
            let pose = XrPose {
                position: Vec3::new(view.pose.position.x, view.pose.position.y, view.pose.position.z),
                orientation: Quat::from_xyzw(view.pose.orientation.x, view.pose.orientation.y, view.pose.orientation.z, view.pose.orientation.w),
            };
            let fov = XrFov {
                left: view.fov.angle_left, right: view.fov.angle_right,
                up: view.fov.angle_up, down: view.fov.angle_down,
            };
            let projection = fov.projection_matrix(engine.camera.near, engine.camera.far)
                .ok_or("invalid OpenXR projection")?;
            engine.camera.position = pose.position;
            engine.camera.target = pose.position + pose.orientation * Vec3::NEG_Z;
            engine.camera.up = pose.orientation * Vec3::Y;
            engine.camera.set_xr_matrices(pose.view_matrix(), projection);
            engine.set_xr_output_target(Some(resource))?;
            let render_result = engine.render_frame();
            engine.set_xr_output_target(None)?;
            eye.swapchain.release_image()?;
            render_result?;
        }
        let layer_views: Vec<_> = views.iter().zip(eyes.iter()).map(|(view, eye)| {
            xr::CompositionLayerProjectionView::new()
                .pose(view.pose)
                .fov(view.fov)
                .sub_image(xr::SwapchainSubImage::new().swapchain(&eye.swapchain).image_rect(rect))
        }).collect();
        let mut layer = xr::CompositionLayerProjection::new().space(space).views(&layer_views);
        if blend == xr::EnvironmentBlendMode::ALPHA_BLEND || passthrough_layer.is_some() {
            layer = layer.layer_flags(
                xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA
                    | xr::CompositionLayerFlags::UNPREMULTIPLIED_ALPHA,
            );
        }
        if let Some(background) = passthrough_layer {
            // openxr 0.22 generates this builder but does not re-export it.
            // The raw struct begins with CompositionLayerBaseHeader as required
            // by the OpenXR ABI and is valid for this end_frame call.
            let background_raw = xr::sys::CompositionLayerPassthroughFB {
                ty: xr::sys::CompositionLayerPassthroughFB::TYPE,
                next: std::ptr::null(),
                flags: xr::CompositionLayerFlags::EMPTY,
                space: space.as_raw(),
                layer_handle: background.as_raw(),
            };
            let background_base = unsafe {
                &*(&background_raw as *const _ as *const xr::CompositionLayerBase<'_, xr::D3D12>)
            };
            let layers: [&xr::CompositionLayerBase<'_, xr::D3D12>; 2] = [background_base, &layer];
            frame_stream.end(frame.predicted_display_time, blend, &layers)?;
        } else {
            frame_stream.end(frame.predicted_display_time, blend, &[&layer])?;
        }
    }
    Ok(())
}
