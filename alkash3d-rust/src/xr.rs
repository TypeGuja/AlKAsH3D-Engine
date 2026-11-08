//! Common camera data for VR and AR runtimes.
//!
//! Coordinates follow the engine's right-handed convention: +X right,
//! +Y up, and -Z forward. Runtime-specific code supplies tracked poses,
//! per-view fields of view and image targets.

use crate::math::{Mat4, Quat, Vec3, Vec4};

#[cfg(feature = "xr")]
pub mod runtime;

/// A tracked pose in metres, relative to the application's XR origin.
#[derive(Clone, Copy, Debug)]
pub struct XrPose {
    pub position: Vec3,
    pub orientation: Quat,
}

impl XrPose {
    pub fn view_matrix(self) -> Mat4 {
        Mat4::from_rotation_translation(self.orientation, self.position).inverse()
    }
}

/// Signed angular limits, in radians, measured from the view's -Z axis.
#[derive(Clone, Copy, Debug)]
pub struct XrFov {
    pub left: f32,
    pub right: f32,
    pub up: f32,
    pub down: f32,
}

impl XrFov {
    /// Right-handed asymmetric projection with DirectX depth in [0, 1].
    pub fn projection_matrix(self, near: f32, far: f32) -> Option<Mat4> {
        if !near.is_finite() || !far.is_finite() || near <= 0.0 || far <= near {
            return None;
        }
        let [left, right, up, down] = [self.left, self.right, self.up, self.down];
        if ![left, right, up, down].iter().all(|angle| angle.is_finite())
            || left >= right || down >= up
            || left <= -std::f32::consts::FRAC_PI_2
            || right >= std::f32::consts::FRAC_PI_2
            || down <= -std::f32::consts::FRAC_PI_2
            || up >= std::f32::consts::FRAC_PI_2
        {
            return None;
        }
        let (l, r, b, t) = (left.tan(), right.tan(), down.tan(), up.tan());
        let width = r - l;
        let height = t - b;
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let depth = far - near;
        Some(Mat4::from_cols(
            Vec4::new(2.0 / width, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 2.0 / height, 0.0, 0.0),
            Vec4::new((r + l) / width, (t + b) / height, -far / depth, -1.0),
            Vec4::new(0.0, 0.0, -near * far / depth, 0.0),
        ))
    }
}

/// One runtime view. Stereo VR supplies two; AR may supply one or two.
#[derive(Clone, Copy, Debug)]
pub struct XrView {
    pub pose: XrPose,
    pub fov: XrFov,
}

impl XrView {
    pub fn view_projection(self, near: f32, far: f32) -> Option<Mat4> {
        Some(self.fov.projection_matrix(near, far)? * self.pose.view_matrix())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symmetric_projection_maps_frustum_edges_and_depth() {
        let fov = XrFov { left: -0.7, right: 0.7, up: 0.6, down: -0.6 };
        let p = fov.projection_matrix(0.1, 100.0).unwrap();
        let near_left = p.project_point3(Vec3::new(-0.1 * 0.7_f32.tan(), 0.0, -0.1));
        let near_right = p.project_point3(Vec3::new(0.1 * 0.7_f32.tan(), 0.0, -0.1));
        let near_top = p.project_point3(Vec3::new(0.0, 0.1 * 0.6_f32.tan(), -0.1));
        let far_center = p.project_point3(Vec3::new(0.0, 0.0, -100.0));
        assert!((near_left.x + 1.0).abs() < 1e-5);
        assert!((near_right.x - 1.0).abs() < 1e-5);
        assert!((near_top.y - 1.0).abs() < 1e-5);
        assert!(near_left.z.abs() < 1e-5);
        assert!((far_center.z - 1.0).abs() < 1e-5);
    }

    #[test]
    fn tracked_pose_changes_view_origin() {
        let pose = XrPose { position: Vec3::new(2.0, 1.0, -3.0), orientation: Quat::IDENTITY };
        assert!(pose.view_matrix().transform_point3(pose.position).length() < 1e-5);
    }

    #[test]
    fn asymmetric_projection_preserves_each_runtime_edge() {
        let fov = XrFov { left: -0.9, right: 0.6, up: 0.7, down: -0.5 };
        let p = fov.projection_matrix(0.2, 50.0).unwrap();
        for (angle, expected) in [(fov.left, -1.0), (fov.right, 1.0)] {
            let point = p.project_point3(Vec3::new(0.2 * angle.tan(), 0.0, -0.2));
            assert!((point.x - expected).abs() < 1e-5);
        }
        for (angle, expected) in [(fov.down, -1.0), (fov.up, 1.0)] {
            let point = p.project_point3(Vec3::new(0.0, 0.2 * angle.tan(), -0.2));
            assert!((point.y - expected).abs() < 1e-5);
        }
    }
}
