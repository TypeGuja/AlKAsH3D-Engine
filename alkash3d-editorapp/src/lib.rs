//! AlKAsH3D Editor Library
//!
//! Библиотека для создания 3D редактора на основе движка alkash3d_rs

pub mod math;
pub mod scene;
pub mod render;
pub mod editor;
pub mod ui;
pub mod assets;
pub mod converters;
pub mod ffi;

// Реэкспорт часто используемых типов
pub use math::{Vec2, Vec3, Vec4, Mat4, Quat, Transform, Camera, Ray, AABB};
pub use scene::{Scene, GameObject, Component};
pub use render::RenderEngine;
pub use editor::{Gizmo, GizmoMode, GizmoSpace, CommandHistory};
pub use assets::AssetLibrary;