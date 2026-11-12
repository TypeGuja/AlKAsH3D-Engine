//! Engine-owned OpenXR session and stereo frame loop for D3D12.

use crate::{
    engine::{AlkashEngine, GraphicsSettings, MeshInstance},
    math::{Quat, Vec3},
    xr::{XrFov, XrPose},
};
use openxr as xr;
use super::input::{ControllerActions, XrInputFrame};
use std::{env, error::Error, ffi::c_void, path::PathBuf, time::Duration};
use windows::{
    core::Interface,
    Win32::{
        Foundation::LUID,
        Graphics::{
            Direct3D::D3D_FEATURE_LEVEL,
            Direct3D12::*,
            Dxgi::Common::{
                DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, DXGI_SAMPLE_DESC,
            },
        },
    },
};

const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;

/// Pose data supplied by OpenXR to an application frame.  Keeping this
/// public separates headset tracking from the demo scene and is the common
/// bridge used by game binaries when they opt into the VR frame loop.
#[derive(Clone, Copy, Debug)]
pub struct XrTrackedView {
    pub position: Vec3,
    pub orientation: Quat,
    pub fov: XrFov,
}

/// Owner of the engine rendered by an OpenXR frame loop.  Most programs use
/// `AlkashEngine` directly; games which keep it inside their own state (such
/// as Buckshot's `Ctx`) can implement this trait and still use the same XR
/// compositor instead of creating a second, fake game state.
pub trait XrEngineOwner {
    fn xr_engine_mut(&mut self) -> &mut AlkashEngine;
}

impl XrEngineOwner for AlkashEngine {
    fn xr_engine_mut(&mut self) -> &mut AlkashEngine { self }
}

/// Loads the Windows OpenXR loader without requiring the user to edit PATH.
///
/// SteamVR keeps `openxr_loader.dll` in its own `bin\\win64` directory,
/// which is normally not on PATH.  The official loader is still used; this
/// merely makes its standard installation location visible to this process.
/// A headset vendor runtime selected in the Windows OpenXR registry remains
/// the runtime that the loader uses.
pub fn load_openxr_entry() -> Result<xr::Entry, Box<dyn Error>> {
    // SAFETY: this asks the official OpenXR loader for its stable C ABI.
    if let Ok(entry) = unsafe { xr::Entry::load(&()) } {
        return Ok(entry);
    }

    let mut candidates = Vec::<PathBuf>::new();
    if let Some(dir) = env::var_os("ALKASH3D_OPENXR_LOADER_DIR") {
        candidates.push(PathBuf::from(dir));
    }
    for variable in ["ProgramFiles(x86)", "ProgramW6432", "ProgramFiles"] {
        if let Some(base) = env::var_os(variable) {
            candidates.push(
                PathBuf::from(base)
                    .join("Steam")
                    .join("steamapps")
                    .join("common")
                    .join("SteamVR")
                    .join("bin")
                    .join("win64"),
            );
        }
    }

    let mut searched = Vec::new();
    for directory in candidates {
        let dll = directory.join("openxr_loader.dll");
        searched.push(dll.display().to_string());
        if !dll.is_file() {
            continue;
        }
        let current_path = env::var_os("PATH").unwrap_or_default();
        let mut paths: Vec<PathBuf> = env::split_paths(&current_path).collect();
        if !paths.iter().any(|path| path == &directory) {
            paths.insert(0, directory.clone());
            let joined = env::join_paths(paths)?;
            // SAFETY: this is called before rendering threads or an OpenXR
            // instance exist, so changing the process environment is safe.
            unsafe {
                env::set_var("PATH", joined);
            }
        }
        // SAFETY: `dll` was checked above and is the official loader shipped
        // by SteamVR.  The loader selects the active vendor runtime itself.
        match unsafe { xr::Entry::load(&()) } {
            Ok(entry) => {
                println!("OpenXR loader: {}", dll.display());
                return Ok(entry);
            }
            Err(error) => {
                return Err(format!(
                    "OpenXR loader was found at {}, but Windows could not load it ({error})",
                    dll.display()
                )
                .into());
            }
        }
    }

    Err(format!(
        "OpenXR loader unavailable. Install SteamVR or set ALKASH3D_OPENXR_LOADER_DIR to the folder containing openxr_loader.dll. Checked: {}",
        searched.join("; ")
    ).into())
}

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
    run_xr_with_frame(ar, setup, |_engine, _views, _dt| Ok(()))
}

/// Same XR session as [`run_xr`], but exposes one callback per predicted XR
/// frame.  Games use this instead of owning a desktop render loop: `tick`
/// updates simulation once, while the runtime renders the two tracked views.
pub fn run_xr_with_frame<F, T>(ar: bool, setup: F, mut tick: T) -> Result<(), Box<dyn Error>>
where
    F: FnOnce(&mut AlkashEngine),
    T: FnMut(&mut AlkashEngine, &[XrTrackedView; 2], f32) -> Result<(), Box<dyn Error>>,
{
    run_xr_with_input(ar, setup, move |engine, views, _input, dt| tick_bridge(engine, views, dt, &mut tick))
}

fn tick_bridge<T>(engine: &mut AlkashEngine, views: &[XrTrackedView; 2], dt: f32, tick: &mut T) -> Result<(), Box<dyn Error>>
where T: FnMut(&mut AlkashEngine, &[XrTrackedView; 2], f32) -> Result<(), Box<dyn Error>> {
    tick(engine, views, dt)
}

/// Frame callback with tracked headset views and two OpenXR controller states.
pub fn run_xr_with_input<F, T>(ar: bool, setup: F, mut tick: T) -> Result<(), Box<dyn Error>>
where
    F: FnOnce(&mut AlkashEngine),
    T: FnMut(&mut AlkashEngine, &[XrTrackedView; 2], &XrInputFrame, f32) -> Result<(), Box<dyn Error>>,
{
    run_xr_with_locomotion(ar, setup, move |engine, views, input, _origin, dt| tick(engine, views, input, dt))
}

/// Frame callback which may move the player/body origin independently of the
/// tracked headset offset.  This is the correct hook for stick locomotion.
pub fn run_xr_with_locomotion<F, T>(ar: bool, setup: F, mut tick: T) -> Result<(), Box<dyn Error>>
where
    F: FnOnce(&mut AlkashEngine),
    T: FnMut(&mut AlkashEngine, &[XrTrackedView; 2], &XrInputFrame, &mut Vec3, f32) -> Result<(), Box<dyn Error>>,
{
    run_xr_with_player(ar, setup, move |engine, views, input, origin, _body_yaw, dt| {
        tick(engine, views, input, origin, dt)
    })
}

/// Extended player callback.  Besides moving the player origin it exposes a
/// body yaw which rotates the world-relative headset pose.  This lets games
/// implement a comfortable snap turn without hijacking physical head motion.
pub fn run_xr_with_player<F, T>(ar: bool, setup: F, tick: T) -> Result<(), Box<dyn Error>>
where
    F: FnOnce(&mut AlkashEngine),
    T: FnMut(&mut AlkashEngine, &[XrTrackedView; 2], &XrInputFrame, &mut Vec3, &mut f32, f32) -> Result<(), Box<dyn Error>>,
{
    run_xr_with_player_state(ar, move |mut engine| {
        setup(&mut engine);
        Ok(engine)
    }, tick)
}

/// Variant of [`run_xr_with_player`] for a game state that owns the engine.
/// It is intentionally public: games retain their real update/animation
/// state and XR only supplies tracking plus stereo presentation.
pub fn run_xr_with_player_state<S, F, T>(ar: bool, setup: F, tick: T) -> Result<(), Box<dyn Error>>
where
    S: XrEngineOwner,
    F: FnOnce(AlkashEngine) -> Result<S, Box<dyn Error>>,
    T: FnMut(&mut S, &[XrTrackedView; 2], &XrInputFrame, &mut Vec3, &mut f32, f32) -> Result<(), Box<dyn Error>>,
{
    let entry = load_openxr_entry()?;
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
    ).map_err(|err| format!(
        "OpenXR runtime did not create an instance ({err}). Start SteamVR (or your headset app), connect the headset, and select its OpenXR runtime."
    ))?;
    let system = instance.system(xr::FormFactor::HEAD_MOUNTED_DISPLAY).map_err(|err| format!(
        "No usable VR headset was reported by OpenXR ({err}). Check that the headset is connected and shown as ready in SteamVR."
    ))?;
    let views = instance.enumerate_view_configuration_views(system, VIEW_TYPE)?;
    if views.len() != 2
        || views[0].recommended_image_rect_width != views[1].recommended_image_rect_width
        || views[0].recommended_image_rect_height != views[1].recommended_image_rect_height
    {
        return Err("this demo requires two equally sized stereo views".into());
    }
    let (width, height) = (
        views[0].recommended_image_rect_width,
        views[0].recommended_image_rect_height,
    );
    let blend_modes = instance.enumerate_environment_blend_modes(system, VIEW_TYPE)?;
    let blend = if ar {
        blend_modes
            .iter()
            .copied()
            .find(|m| *m == xr::EnvironmentBlendMode::ALPHA_BLEND)
            .or_else(|| {
                blend_modes
                    .iter()
                    .copied()
                    .find(|m| *m == xr::EnvironmentBlendMode::ADDITIVE)
            })
            .or_else(|| {
                if enabled.fb_passthrough {
                    blend_modes
                        .iter()
                        .copied()
                        .find(|m| *m == xr::EnvironmentBlendMode::OPAQUE)
                } else {
                    None
                }
            })
            .ok_or("the active OpenXR runtime offers no AR environment blend mode")?
    } else {
        blend_modes
            .iter()
            .copied()
            .find(|m| *m == xr::EnvironmentBlendMode::OPAQUE)
            .ok_or("the active OpenXR runtime offers no opaque VR blend mode")?
    };
    let use_fb_passthrough = ar && blend == xr::EnvironmentBlendMode::OPAQUE;
    let requirements = instance.graphics_requirements::<xr::D3D12>(system)?;
    let luid = LUID {
        LowPart: requirements.adapter_luid.LowPart,
        HighPart: requirements.adapter_luid.HighPart,
    };
    let mut engine = AlkashEngine::new(width, height);
    engine.set_graphics_settings(GraphicsSettings {
        msaa: false,
        ssao: false,
        bloom: false,
        volumetric: false,
        shadows: false,
        render_scale: 1.0,
    });
    engine.init_for_xr(luid, D3D_FEATURE_LEVEL(requirements.min_feature_level))?;
    engine.process_messages();
    let renderer_size = engine
        .renderer
        .as_ref()
        .map(|renderer| (renderer.width, renderer.height));
    if renderer_size != Some((width, height)) {
        return Err(format!(
            "XR mirror size mismatch: expected {width}x{height}, got {renderer_size:?}"
        )
        .into());
    }
    engine.set_xr_alpha_blend(blend == xr::EnvironmentBlendMode::ALPHA_BLEND || use_fb_passthrough);
    engine.set_clear_color(0.0, 0.0, 0.0, if ar { 0.0 } else { 1.0 });
    let mut state = setup(engine)?;
    // The setup camera is the game-space tracking origin (spawn point,
    // cabin seat, etc.); OpenXR poses are local offsets in metres.
    let tracking_origin = state.xr_engine_mut().camera.position;

    let device = crate::get_device()?;
    let queue = crate::get_command_queue()?;
    let graphics = xr::d3d::SessionCreateInfoD3D12 {
        device: device.as_raw() as _,
        queue: queue.as_raw() as _,
    };
    // SAFETY: the engine device uses the runtime's LUID and minimum feature
    // level; both COM objects remain alive until the session is dropped.
    let (session, mut frame_waiter, mut frame_stream) =
        unsafe { instance.create_session::<xr::D3D12>(system, &graphics)? };
    let space =
        session.create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)?;
    let controllers = ControllerActions::new(&instance, &session)?;
    // The creation flag starts both objects without calling Passthrough::start,
    // which is implemented incorrectly in openxr 0.22 (it calls pause).
    let passthrough = if use_fb_passthrough {
        Some(session.create_passthrough(xr::PassthroughFlagsFB::IS_RUNNING_AT_CREATION)?)
    } else {
        None
    };
    let passthrough_layer = if let Some(passthrough) = &passthrough {
        Some(session.create_passthrough_layer(
            passthrough,
            xr::PassthroughFlagsFB::IS_RUNNING_AT_CREATION,
            xr::PassthroughLayerPurposeFB::RECONSTRUCTION,
        )?)
    } else {
        None
    };
    let formats = session.enumerate_swapchain_formats()?;
    let format = [
        DXGI_FORMAT_R8G8B8A8_UNORM_SRGB.0 as u32,
        DXGI_FORMAT_R8G8B8A8_UNORM.0 as u32,
    ]
    .into_iter()
    .find(|format| formats.contains(format))
    .ok_or("OpenXR runtime does not accept an R8G8B8A8 color format")?;
    let mut eyes = Vec::with_capacity(2);
    for _ in 0..2 {
        let swapchain = session.create_swapchain(&xr::SwapchainCreateInfo {
            create_flags: xr::SwapchainCreateFlags::EMPTY,
            usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                | xr::SwapchainUsageFlags::TRANSFER_DST,
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
    println!(
        "OpenXR {:?}: {}x{} per eye, {:?} blend",
        instance.properties()?.runtime_name,
        width,
        height,
        blend
    );

    let outcome = run(
        &instance,
        &session,
        &space,
        &controllers,
        passthrough_layer.as_ref(),
        &mut frame_waiter,
        &mut frame_stream,
        &mut eyes,
        &mut state,
        blend,
        width,
        height,
        tracking_origin,
        tick,
    );
    state.xr_engine_mut().set_xr_output_target(None)?;
    state.xr_engine_mut().camera.clear_xr_matrices();
    state.xr_engine_mut().shutdown();
    outcome
}

/// Checks the same D3D12 copy path with a local texture when no XR runtime
/// or headset is installed. It does not simulate tracking or composition.
pub fn run_mock_copy() -> Result<(), Box<dyn Error>> {
    let (width, height) = (640, 480);
    let mut engine = AlkashEngine::new(width, height);
    engine.set_graphics_settings(GraphicsSettings {
        msaa: false,
        ssao: false,
        bloom: false,
        volumetric: false,
        shadows: false,
        render_scale: 1.0,
    });
    engine.init()?;
    engine.process_messages();
    let (actual_width, actual_height) = engine
        .renderer
        .as_ref()
        .map(|renderer| (renderer.width, renderer.height))
        .ok_or("renderer unavailable after initialization")?;
    let cube = engine.add_cube_colored(0.7, 0.2, 0.8, 0.8, 1.0);
    engine
        .mesh_instances
        .push(MeshInstance::new(cube).at(0.0, 0.0, 0.0));
    let device = crate::get_device()?;
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Alignment: 0,
        Width: actual_width as u64,
        Height: actual_height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
        Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
    };
    let mut texture: Option<ID3D12Resource> = None;
    unsafe {
        device.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            None,
            &mut texture,
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
fn run<S: XrEngineOwner>(
    instance: &xr::Instance,
    session: &xr::Session<xr::D3D12>,
    space: &xr::Space,
    controllers: &ControllerActions,
    passthrough_layer: Option<&xr::PassthroughLayerFB>,
    frame_waiter: &mut xr::FrameWaiter,
    frame_stream: &mut xr::FrameStream<xr::D3D12>,
    eyes: &mut [Eye],
    state: &mut S,
    blend: xr::EnvironmentBlendMode,
    width: u32,
    height: u32,
    mut tracking_origin: Vec3,
    mut tick: impl FnMut(&mut S, &[XrTrackedView; 2], &XrInputFrame, &mut Vec3, &mut f32, f32) -> Result<(), Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    let mut events = xr::EventDataBuffer::new();
    let mut running = false;
    let mut previous_time = std::time::Instant::now();
    let mut body_yaw = 0.0f32;
    loop {
        state.xr_engine_mut().process_messages();
        if !state.xr_engine_mut().is_running() {
            if running {
                session.request_exit()?;
            }
            break;
        }
        while let Some(event) = instance.poll_event(&mut events)? {
            match event {
                xr::Event::SessionStateChanged(change) => match change.state() {
                    xr::SessionState::READY => {
                        session.begin(VIEW_TYPE)?;
                        running = true;
                    }
                    xr::SessionState::STOPPING => {
                        session.end()?;
                        running = false;
                    }
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
        let (flags, views) =
            session.locate_views(VIEW_TYPE, frame.predicted_display_time, space)?;
        if views.len() != 2
            || !flags.contains(
                xr::ViewStateFlags::POSITION_VALID | xr::ViewStateFlags::ORIENTATION_VALID,
            )
        {
            frame_stream.end(frame.predicted_display_time, blend, &[])?;
            continue;
        }
        let body_rotation = Quat::from_rotation_y(body_yaw);
        let tracked = std::array::from_fn(|i| {
            let view = views[i];
            XrTrackedView {
                position: Vec3::new(view.pose.position.x, view.pose.position.y, view.pose.position.z),
                orientation: body_rotation * Quat::from_xyzw(view.pose.orientation.x, view.pose.orientation.y, view.pose.orientation.z, view.pose.orientation.w),
                fov: XrFov { left: view.fov.angle_left, right: view.fov.angle_right, up: view.fov.angle_up, down: view.fov.angle_down },
            }
        });
        let now = std::time::Instant::now();
        let dt = now.duration_since(previous_time).as_secs_f32().min(0.1);
        previous_time = now;
        let input = controllers.sample(session, space, frame.predicted_display_time)?;
        tick(state, &tracked, &input, &mut tracking_origin, &mut body_yaw, dt)?;
        let rect = xr::Rect2Di {
            offset: xr::Offset2Di { x: 0, y: 0 },
            extent: xr::Extent2Di {
                width: width as _,
                height: height as _,
            },
        };
        for (eye, view) in eyes.iter_mut().zip(&views) {
            let index = eye.swapchain.acquire_image()? as usize;
            eye.swapchain.wait_image(xr::Duration::INFINITE)?;
            let raw = eye.images[index] as *mut c_void;
            // SAFETY: the acquired image stays owned by this swapchain until
            // release_image; clone obtains a COM reference for the renderer.
            let resource: ID3D12Resource = unsafe { ID3D12Resource::from_raw_borrowed(&raw) }
                .ok_or("OpenXR returned a null D3D12 swapchain image")?
                .clone();
            let body_rotation = Quat::from_rotation_y(body_yaw);
            let pose = XrPose {
                position: tracking_origin + body_rotation * Vec3::new(
                    view.pose.position.x,
                    view.pose.position.y,
                    view.pose.position.z,
                ),
                orientation: body_rotation * Quat::from_xyzw(
                    view.pose.orientation.x,
                    view.pose.orientation.y,
                    view.pose.orientation.z,
                    view.pose.orientation.w,
                ),
            };
            let fov = XrFov {
                left: view.fov.angle_left,
                right: view.fov.angle_right,
                up: view.fov.angle_up,
                down: view.fov.angle_down,
            };
            let (near, far) = {
                let engine = state.xr_engine_mut();
                (engine.camera.near, engine.camera.far)
            };
            let projection = fov
                .projection_matrix(near, far)
                .ok_or("invalid OpenXR projection")?;
            let engine = state.xr_engine_mut();
            engine.camera.position = pose.position;
            engine.camera.target = engine.camera.position + pose.orientation * Vec3::NEG_Z;
            engine.camera.up = pose.orientation * Vec3::Y;
            engine
                .camera
                .set_xr_matrices(pose.view_matrix(), projection);
            engine.set_xr_output_target(Some(resource))?;
            let render_result = engine.render_frame();
            engine.set_xr_output_target(None)?;
            eye.swapchain.release_image()?;
            render_result?;
        }
        let layer_views: Vec<_> = views
            .iter()
            .zip(eyes.iter())
            .map(|(view, eye)| {
                xr::CompositionLayerProjectionView::new()
                    .pose(view.pose)
                    .fov(view.fov)
                    .sub_image(
                        xr::SwapchainSubImage::new()
                            .swapchain(&eye.swapchain)
                            .image_rect(rect),
                    )
            })
            .collect();
        let mut layer = xr::CompositionLayerProjection::new()
            .space(space)
            .views(&layer_views);
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
