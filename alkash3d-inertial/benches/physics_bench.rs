// benches/physics_bench.rs
use criterion::{criterion_group, criterion_main, Criterion};
use alkash3d_inertial::*;

fn bench_physics_1000(c: &mut Criterion) {
    c.bench_function("physics_1000_bodies", |b| {
        b.iter(|| {
            let mut world = PhysicsWorld::new();
            for i in 0..1000 {
                let body = RigidBody::new(1.0, Vector3::new(i as f32, 10.0, 0.0));
                world.add_body(body);
            }
            for _ in 0..60 {
                world.update(1.0 / 60.0);
            }
        });
    });
}

fn bench_collision_detection(c: &mut Criterion) {
    c.bench_function("collision_detection_1000", |b| {
        b.iter(|| {
            let mut world = PhysicsWorld::new();
            for i in 0..1000 {
                let body = RigidBody::new(1.0, Vector3::new(i as f32, 0.0, 0.0));
                world.add_body(body);
            }
            world.update(1.0 / 60.0);
        });
    });
}

criterion_group!(benches, bench_physics_1000, bench_collision_detection);
criterion_main!(benches);