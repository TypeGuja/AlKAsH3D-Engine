//! Simple controller interaction edges and aim rays (no scene-specific physics).
use super::input::{HandInput, XrInputFrame};

#[derive(Clone, Copy, Debug, Default)]
pub struct InteractionHand {
    pub pressed: bool,
    pub released: bool,
    pub holding: bool,
    pub ray_origin: [f32; 3],
    pub ray_direction: [f32; 3],
    pub ray_valid: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct InteractionFrame {
    pub left: InteractionHand,
    pub right: InteractionHand,
}

#[derive(Default)]
pub struct InteractionTracker {
    previous: [bool; 2],
}

impl InteractionTracker {
    pub fn update(&mut self, input: &XrInputFrame) -> InteractionFrame {
        let left = self.hand(0, &input.left);
        let right = self.hand(1, &input.right);
        InteractionFrame { left, right }
    }

    fn hand(&mut self, index: usize, input: &HandInput) -> InteractionHand {
        let held = input.select;
        let prev = std::mem::replace(&mut self.previous[index], held);
        let q = input.aim.orientation;
        // Rotate local -Z by quaternion (x,y,z,w).
        let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
        let direction = [
            -2.0 * (x * z + w * y),
            -2.0 * (y * z - w * x),
            -1.0 + 2.0 * (x * x + y * y),
        ];
        InteractionHand {
            pressed: held && !prev,
            released: !held && prev,
            holding: held,
            ray_origin: input.aim.position,
            ray_direction: direction,
            ray_valid: input.aim.valid,
        }
    }
}
