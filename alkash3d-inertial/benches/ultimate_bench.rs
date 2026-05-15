// benches/ultimate_bench.rs
// ПОЛНЫЙ ТЕСТ ПРОИЗВОДИТЕЛЬНОСТИ

use criterion::{criterion_group, criterion_main, Criterion};
use alkash3d_inertial::*;
use std::time::Instant;

fn bench_all_modules(c: &mut Criterion) {
    let mut group = c.benchmark_group("Inertial_Ultimate");

    // Тест 1: Только интеграция
    group.bench_function("integration_1M", |b| {
        b.iter(|| {
            let mut world = PhysicsWorld::with_capacity(1_000_000);
            for i in 0..1_000_000 {
                let body = RigidBody::new(1.0, Vector3::new(i as f32 % 1000.0, 10.0, (i / 1000) as f32));
                world.add_body(body);
            }
            world.update(1.0 / 60.0);
        });
    });

    // Тест 2: Broad phase
    group.bench_function("broad_phase_1M", |b| {
        b.iter(|| {
            let mut world = PhysicsWorld::with_capacity(1_000_000).with_collisions(true);
            for i in 0..1_000_000 {
                let body = RigidBody::new(1.0, Vector3::new(i as f32 % 1000.0, 10.0, (i / 1000) as f32));
                world.add_body(body);
            }
            world.update(1.0 / 60.0);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_all_modules);
criterion_main!(benches);