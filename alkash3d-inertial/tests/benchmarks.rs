#![feature(test)]
extern crate test;

#[cfg(test)]
mod benchmarks {
    use test::Bencher;
    use inertial::*;

    #[bench]
    fn bench_rigid_body_update(b: &mut Bencher) {
        let mut world = PhysicsWorld::new();

        // Добавляем 1000 тел
        for i in 0..1000 {
            let body = RigidBody::new(
                10.0,
                Vector3::new(i as f32, 0.0, 0.0),
            );
            world.add_body(body);
        }

        b.iter(|| {
            world.update(1.0 / 60.0);
        });
    }

    #[bench]
    fn bench_collision_detection(b: &mut Bencher) {
        let pos1 = Vector3::new(0.0, 0.0, 0.0);
        let pos2 = Vector3::new(0.8, 0.0, 0.0);

        b.iter(|| {
            for _ in 0..10000 {
                let _ = CollisionDetector::sphere_sphere(pos1, 0.5, pos2, 0.5);
            }
        });
    }

    #[bench]
    fn bench_aabb_intersection(b: &mut Bencher) {
        let a_min = Vector3::new(-1.0, -1.0, -1.0);
        let a_max = Vector3::new(1.0, 1.0, 1.0);
        let b_min = Vector3::new(0.5, 0.5, 0.5);
        let b_max = Vector3::new(2.0, 2.0, 2.0);

        b.iter(|| {
            for _ in 0..100000 {
                let _ = CollisionDetector::aabb_aabb(a_min, a_max, b_min, b_max);
            }
        });
    }
}