// src/lib.rs
#![allow(dead_code)]
#![allow(unused_imports)]

pub mod math;
pub mod world;
pub mod body;
pub mod collision;
pub mod solver;
pub mod simd_math;

// FFI модуль (только если есть Fortran)
#[cfg(feature = "fortran")]
pub mod ffi;

pub use math::*;
pub use world::*;
pub use body::*;
pub use collision::*;
pub use solver::*;

pub type Vector3 = nalgebra::Vector3<f32>;
pub type Point3 = nalgebra::Point3<f32>;
pub type Matrix4 = nalgebra::Matrix4<f32>;

// Версия движка
pub const VERSION: &str = env!("CARGO_PKG_VERSION");