// src/lib.rs
#![allow(dead_code)]
#![allow(unused_imports)]

pub mod math;
pub mod world;
pub mod body;
pub mod collision;
pub mod solver;

pub use math::*;
pub use world::*;
pub use body::*;
pub use collision::*;
pub use solver::*;

pub type Vector3 = nalgebra::Vector3<f32>;
pub type Point3 = nalgebra::Point3<f32>;
pub type Matrix4 = nalgebra::Matrix4<f32>;