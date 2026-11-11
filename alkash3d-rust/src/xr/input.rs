//! OpenXR controller actions and per-frame state.
use openxr as xr;
use std::error::Error;

#[derive(Clone, Copy, Debug, Default)]
pub struct HandPose {
    pub position: [f32; 3],
    pub orientation: [f32; 4],
    pub valid: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HandInput {
    pub aim: HandPose,
    pub grip_pose: HandPose,
    pub trigger: f32,
    pub squeeze: f32,
    pub thumbstick: [f32; 2],
    pub select: bool,
    pub menu: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct XrInputFrame {
    pub left: HandInput,
    pub right: HandInput,
}

pub struct ControllerActions {
    set: xr::ActionSet,
    hands: [xr::Path; 2],
    aim: xr::Action<xr::Posef>,
    grip_pose: xr::Action<xr::Posef>,
    trigger: xr::Action<f32>,
    squeeze: xr::Action<f32>,
    stick: xr::Action<xr::Vector2f>,
    select: xr::Action<bool>,
    menu: xr::Action<bool>,
    aim_spaces: [xr::Space; 2],
    grip_spaces: [xr::Space; 2],
}

impl ControllerActions {
    pub fn new(instance: &xr::Instance, session: &xr::Session<xr::D3D12>) -> Result<Self, Box<dyn Error>> {
        let hands = [instance.string_to_path("/user/hand/left")?, instance.string_to_path("/user/hand/right")?];
        let set = instance.create_action_set("alkash_gameplay", "AlKAsH Gameplay", 0)?;
        let aim = set.create_action::<xr::Posef>("aim_pose", "Aim Pose", &hands)?;
        let grip_pose = set.create_action::<xr::Posef>("grip_pose", "Grip Pose", &hands)?;
        let trigger = set.create_action::<f32>("trigger_value", "Trigger", &hands)?;
        let squeeze = set.create_action::<f32>("squeeze_value", "Grip", &hands)?;
        let stick = set.create_action::<xr::Vector2f>("thumbstick", "Thumbstick", &hands)?;
        let select = set.create_action::<bool>("select_click", "Select", &hands)?;
        let menu = set.create_action::<bool>("menu_click", "Menu", &hands)?;

        // Bind common controller profiles; unsupported profiles are skipped.
        for (profile, is_simple) in [
            ("/interaction_profiles/khr/simple_controller", true),
            ("/interaction_profiles/oculus/touch_controller", false),
            ("/interaction_profiles/valve/index_controller", false),
            ("/interaction_profiles/htc/vive_controller", false),
        ] {
            let mut bindings = Vec::new();
            for side in ["left", "right"] {
                let base = format!("/user/hand/{side}/input");
                bindings.push(xr::Binding::new(&aim, instance.string_to_path(&format!("{base}/aim/pose"))?));
                bindings.push(xr::Binding::new(&grip_pose, instance.string_to_path(&format!("{base}/grip/pose"))?));
            }
            // Different action types cannot share one homogeneous Binding vector's generic
            // lifetime problem? Binding is type-erased in openxr, so append freely.
            if is_simple {
                for side in ["left", "right"] {
                    let base = format!("/user/hand/{side}/input");
                    bindings.push(xr::Binding::new(&select, instance.string_to_path(&format!("{base}/select/click"))?));
                }
            } else {
                for side in ["left", "right"] {
                    let base = format!("/user/hand/{side}/input");
                    bindings.push(xr::Binding::new(&trigger, instance.string_to_path(&format!("{base}/trigger/value"))?));
                    bindings.push(xr::Binding::new(&squeeze, instance.string_to_path(&format!("{base}/squeeze/value"))?));
                    if profile != "/interaction_profiles/htc/vive_controller" {
                        bindings.push(xr::Binding::new(&stick, instance.string_to_path(&format!("{base}/thumbstick"))?));
                    }
                    bindings.push(xr::Binding::new(&select, instance.string_to_path(&format!("{base}/trigger/click"))?));
                }
            }
            if let Ok(path) = instance.string_to_path(profile) {
                if let Err(err) = instance.suggest_interaction_profile_bindings(path, &bindings) {
                    eprintln!("OpenXR bindings for {profile}: {err}");
                }
            }
        }
        session.attach_action_sets(&[&set])?;
        let aim_spaces = [
            aim.create_space(session, hands[0], xr::Posef::IDENTITY)?,
            aim.create_space(session, hands[1], xr::Posef::IDENTITY)?,
        ];
        let grip_spaces = [
            grip_pose.create_space(session, hands[0], xr::Posef::IDENTITY)?,
            grip_pose.create_space(session, hands[1], xr::Posef::IDENTITY)?,
        ];
        Ok(Self { set, hands, aim, grip_pose, trigger, squeeze, stick, select, menu, aim_spaces, grip_spaces })
    }

    pub fn sample(&self, session: &xr::Session<xr::D3D12>, origin: &xr::Space, time: xr::Time) -> Result<XrInputFrame, Box<dyn Error>> {
        session.sync_actions(&[xr::ActiveActionSet::new(&self.set)])?;
        let mut out = XrInputFrame::default();
        for (i, hand) in [&mut out.left, &mut out.right].into_iter().enumerate() {
            let path = self.hands[i];
            hand.aim = locate(&self.aim_spaces[i], origin, time)?;
            hand.grip_pose = locate(&self.grip_spaces[i], origin, time)?;
            hand.trigger = self.trigger.state(session, path)?.current_state;
            hand.squeeze = self.squeeze.state(session, path)?.current_state;
            let stick = self.stick.state(session, path)?.current_state;
            hand.thumbstick = [stick.x, stick.y];
            hand.select = self.select.state(session, path)?.current_state || hand.trigger > 0.8;
            hand.menu = self.menu.state(session, path)?.current_state;
        }
        Ok(out)
    }
}

fn locate(space: &xr::Space, origin: &xr::Space, time: xr::Time) -> Result<HandPose, Box<dyn Error>> {
    let location = space.locate(origin, time)?;
    let valid = location.location_flags.contains(
        xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID,
    );
    let p = location.pose;
    Ok(HandPose {
        position: [p.position.x, p.position.y, p.position.z],
        orientation: [p.orientation.x, p.orientation.y, p.orientation.z, p.orientation.w],
        valid,
    })
}
