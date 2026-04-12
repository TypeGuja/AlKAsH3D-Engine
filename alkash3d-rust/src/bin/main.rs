// src/bin/main.rs
use alkash3d_rs::*;

fn main() {
    println!("=== Alkash3D Engine Test ===\n");

    // Тест .altex формата
    let mut altex = AltexFile::new();
    let mesh_id = altex.add_mesh(vec![], vec![], "TestMesh");
    println!("✓ Created .altex with mesh ID: {}", mesh_id);

    // Тест .alfar формата
    let night_city = AlfarFile::create_night_city();
    println!("✓ Created .alfar with {} lights", night_city.lights.len());

    // Тест .alcar формата
    let police_car = AlcarFile::create_police_car();
    println!("✓ Created police car (category: {})", police_car.metadata.category);

    let sports_car = AlcarFile::create_sports_car();
    println!("✓ Created sports car (top speed: {} km/h)", sports_car.physics.top_speed);

    // Тест .alroute формата
    let mut route = AlrouteFile::new();
    let waypoints = vec![
        Waypoint { position: [0.0, 0.0, 0.0], wait_time: 0.0, speed_limit: 50.0, action_id: 0 },
        Waypoint { position: [10.0, 0.0, 0.0], wait_time: 0.0, speed_limit: 50.0, action_id: 0 },
        Waypoint { position: [20.0, 0.0, 0.0], wait_time: 1.0, speed_limit: 30.0, action_id: 1 },
    ];
    route.add_route("TestRoute", &waypoints, 1);
    println!("✓ Created .alroute with {} waypoints", route.waypoints.len());

    // Сохраняем тестовые файлы
    let _ = altex.save("test.altex");
    let _ = night_city.save("test.alfar");
    let _ = police_car.save("test_police.alcar");
    let _ = sports_car.save("test_sport.alcar");
    let _ = route.save("test.alroute");

    println!("\n=== All tests passed! ===");
    println!("Created files: test.altex, test.alfar, test_police.alcar, test_sport.alcar, test.alroute");
}