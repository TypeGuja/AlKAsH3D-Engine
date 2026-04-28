use crate::math::{Vec3, Transform};
use rayon::prelude::*;
use rand;

#[derive(Debug, Clone)]
pub struct Particle {
    pub position: Vec3,
    pub velocity: Vec3,
    pub lifetime: f32,
    pub max_lifetime: f32,
    pub size: f32,
    pub color: [f32; 4],
}

#[derive(Debug, Clone)]
pub struct ParticleSystem {
    pub particles: Vec<Particle>,
    pub emission_rate: f32,
    pub emission_timer: f32,
    pub max_particles: usize,
    pub gravity: Vec3,
    pub start_color: [f32; 4],
    pub end_color: [f32; 4],
    pub start_size: f32,
    pub end_size: f32,
    pub lifetime: f32,
    pub velocity: Vec3,
    pub velocity_random: f32,
    pub enabled: bool,
    pub looping: bool,
    pub transform: Transform,
}

impl ParticleSystem {
    pub fn new() -> Self {
        Self {
            particles: Vec::new(),
            emission_rate: 10.0,
            emission_timer: 0.0,
            max_particles: 1000,
            gravity: Vec3::new(0.0, -9.81, 0.0),
            start_color: [1.0, 1.0, 1.0, 1.0],
            end_color: [1.0, 1.0, 1.0, 0.0],
            start_size: 0.1,
            end_size: 0.0,
            lifetime: 2.0,
            velocity: Vec3::new(0.0, 5.0, 0.0),
            velocity_random: 1.0,
            enabled: true,
            looping: true,
            transform: Transform::default(),
        }
    }

    pub fn update(&mut self, delta_time: f32) {
        if !self.enabled { return; }

        self.emission_timer += delta_time;
        let particles_to_emit = (self.emission_timer * self.emission_rate) as usize;
        self.emission_timer = self.emission_timer.fract() / self.emission_rate;

        let remaining = self.max_particles - self.particles.len();
        for _ in 0..particles_to_emit.min(remaining) {
            self.particles.push(Particle {
                position: self.transform.position,
                velocity: self.velocity + Vec3::new(
                    rand::random::<f32>() - 0.5,
                    rand::random::<f32>() - 0.5,
                    rand::random::<f32>() - 0.5,
                ) * self.velocity_random,
                lifetime: 0.0,
                max_lifetime: self.lifetime,
                size: self.start_size,
                color: self.start_color,
            });
        }

        self.particles.par_iter_mut().for_each(|p| {
            p.lifetime += delta_time;
            p.velocity += self.gravity * delta_time;
            p.position += p.velocity * delta_time;
            let t = p.lifetime / p.max_lifetime;
            p.size = self.start_size + (self.end_size - self.start_size) * t;
            for i in 0..4 {
                p.color[i] = self.start_color[i] + (self.end_color[i] - self.start_color[i]) * t;
            }
        });

        self.particles.retain(|p| p.lifetime < p.max_lifetime);

        if !self.looping && self.particles.is_empty() {
            self.enabled = false;
        }
    }
}