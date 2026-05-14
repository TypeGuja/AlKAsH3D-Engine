#[cfg(test)]
mod tests {
    use inertial::*;

    #[test]
    fn test_rigid_body_creation() {
        let body = RigidBody::new(10.0, Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(body.mass, 10.0);
        assert_eq!(body.inv_mass, 0.1);
        assert!(!body.is_static);
    }

    #[test]
    fn test_apply_force() {
        let mut body = RigidBody::new(10.0, Vector3::zeros());
        body.apply_force_center(Vector3::new(100.0, 0.0, 0.0));

        // Сила должна накопиться
        assert_eq!(body.force_accumulator.x, 100.0);
    }

    #[test]
    fn test_collision_sphere_sphere() {
        let pos1 = Vector3::new(0.0, 0.0, 0.0);
        let pos2 = Vector3::new(1.0, 0.0, 0.0);

        let manifold = CollisionDetector::sphere_sphere(pos1, 0.5, pos2, 0.5);
        assert!(manifold.is_some());

        let manifold = manifold.unwrap();
        assert!(manifold.penetration > 0.0);
    }

    #[test]
    fn test_physics_world() {
        let mut world = PhysicsWorld::new();

        let body = RigidBody::new(10.0, Vector3::new(0.0, 10.0, 0.0));
        world.add_body(body);

        // Симулируем 1 секунду
        for _ in 0..60 {
            world.update(1.0 / 60.0);
        }

        let stats = world.get_stats();
        assert!(stats.bodies_count > 0);
    }

    #[test]
    fn test_aabb_collision() {
        let a_min = Vector3::new(-1.0, -1.0, -1.0);
        let a_max = Vector3::new(1.0, 1.0, 1.0);

        let b_min = Vector3::new(0.5, 0.5, 0.5);
        let b_max = Vector3::new(2.0, 2.0, 2.0);

        let manifold = CollisionDetector::aabb_aabb(a_min, a_max, b_min, b_max);
        assert!(manifold.is_some());
    }

    #[test]
    fn test_sleeping() {
        let mut body = RigidBody::new(10.0, Vector3::zeros());
        body.velocity = Vector3::new(0.001, 0.001, 0.001);

        // Должен заснуть через 2 секунды
        for _ in 0..120 {
            if body.can_sleep(1.0 / 60.0) && body.sleep_timer > 2.0 {
                body.is_asleep = true;
                break;
            }
        }

        assert!(body.is_asleep);
    }
}