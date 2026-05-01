// src/ui/viewport.rs
use egui::*;
use crate::math::Vec3;
use crate::scene::ObjectType;

pub fn render_viewport(ui: &mut Ui, app: &mut crate::EditorApp) {
    let rect = ui.available_rect_before_wrap();
    app.viewport_rect = rect;

    // Если есть GPU - показываем bounding box'ы и информацию
    if app.use_gpu && app.gpu_renderer.is_some() {
        let bg = Color32::from_rgb(
            (app.scene.ambient_color[0] * 255.0) as u8,
            (app.scene.ambient_color[1] * 255.0) as u8,
            (app.scene.ambient_color[2] * 255.0) as u8
        );
        ui.painter().rect_filled(rect, 0.0, bg);

        // Рисуем bounding box'ы для всех объектов
        let objects: Vec<_> = app.scene.objects.values().collect();
        for obj in &objects {
            if !obj.visible { continue; }
            let world = app.scene.get_world_transform(obj.id);
            let selected = app.scene.selected_ids.contains(&obj.id);

            match &obj.object_type {
                ObjectType::Mesh(m) => {
                    if m.solid || m.wireframe {
                        render_bounding_box(ui, app, &m.mesh, &world, selected, rect);
                    }
                }
                ObjectType::Light(l) => {
                    if let Some(pos) = app.world_to_screen(world.position, rect) {
                        let col = Color32::from_rgb((l.color[0]*255.0) as u8, (l.color[1]*255.0) as u8, (l.color[2]*255.0) as u8);
                        ui.painter().circle(pos, 15.0, col, (2.0, Color32::WHITE));
                    }
                }
                _ => {}
            }

            if let Some(pos) = app.world_to_screen(world.position + Vec3::UP * 1.0, rect) {
                ui.painter().text(pos, Align2::CENTER_CENTER, &obj.name,
                                  FontId::proportional(10.0),
                                  if selected { Color32::WHITE } else { Color32::LIGHT_GRAY });
            }
        }

        // Информация GPU
        ui.painter().text(
            Pos2::new(rect.right() - 10.0, rect.top() + 10.0),
            Align2::RIGHT_TOP,
            &format!("GPU: {:.1} FPS", app.fps),
            FontId::proportional(12.0),
            Color32::GREEN,
        );
        return;
    }

    // CPU рендеринг
    let bg = Color32::from_rgb(
        (app.scene.ambient_color[0] * 255.0) as u8,
        (app.scene.ambient_color[1] * 255.0) as u8,
        (app.scene.ambient_color[2] * 255.0) as u8
    );
    ui.painter().rect_filled(rect, 0.0, bg);

    if app.scene.grid_enabled {
        let grid_color = Color32::from_rgb(60, 60, 70);
        for i in -20..=20 {
            for j in -20..=20 {
                let x = i as f32; let z = j as f32;
                if let (Some(p1), Some(p2)) = (app.world_to_screen(Vec3::new(x, 0.0, z), rect), app.world_to_screen(Vec3::new(x + 1.0, 0.0, z), rect)) { ui.painter().line_segment([p1, p2], (1.0, grid_color)); }
                if let (Some(p1), Some(p3)) = (app.world_to_screen(Vec3::new(x, 0.0, z), rect), app.world_to_screen(Vec3::new(x, 0.0, z + 1.0), rect)) { ui.painter().line_segment([p1, p3], (1.0, grid_color)); }
            }
        }
    }

    let objects: Vec<_> = app.scene.objects.values().collect();
    for obj in &objects {
        if !obj.visible { continue; }
        let world = app.scene.get_world_transform(obj.id);
        let selected = app.scene.selected_ids.contains(&obj.id);

        match &obj.object_type {
            ObjectType::Mesh(m) => {
                if m.solid || m.wireframe {
                    let triangle_count = m.mesh.indices.len() / 3;
                    if triangle_count > app.cpu_render_limit {
                        render_bounding_box(ui, app, &m.mesh, &world, selected, rect);
                    } else {
                        render_mesh(ui, app, &m.mesh, &m.material, &world, m.wireframe, selected, rect, m.double_sided);
                    }
                }
            }
            ObjectType::Light(l) => {
                if let Some(pos) = app.world_to_screen(world.position, rect) {
                    let col = Color32::from_rgb((l.color[0]*255.0) as u8, (l.color[1]*255.0) as u8, (l.color[2]*255.0) as u8);
                    ui.painter().circle(pos, 15.0, col, (2.0, Color32::WHITE));
                }
            }
            _ => {}
        }
        if let Some(pos) = app.world_to_screen(world.position + Vec3::UP * 1.0, rect) {
            ui.painter().text(pos, Align2::CENTER_CENTER, &obj.name,
                              FontId::proportional(10.0), if selected { Color32::WHITE } else { Color32::LIGHT_GRAY });
        }
    }
}

fn render_bounding_box(ui: &Ui, app: &crate::EditorApp, mesh: &crate::mesh::Mesh, transform: &crate::math::Transform, selected: bool, rect: Rect) {
    let (min, max) = mesh.bounds;
    let corners = [
        Vec3::new(min.x, min.y, min.z), Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, max.y, min.z), Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z), Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, max.z), Vec3::new(min.x, max.y, max.z),
    ];
    let transformed: Vec<Pos2> = corners.iter().filter_map(|c| {
        app.world_to_screen(transform.transform_point(*c), rect)
    }).collect();
    if transformed.len() < 8 { return; }
    let color = if selected { Color32::from_rgb(255, 200, 100) } else { Color32::from_rgb(255, 255, 0) };
    let edges = [(0,1),(1,2),(2,3),(3,0),(4,5),(5,6),(6,7),(7,4),(0,4),(1,5),(2,6),(3,7)];
    for &(a, b) in &edges { ui.painter().line_segment([transformed[a], transformed[b]], (1.5, color)); }
    if let Some(center) = app.world_to_screen(transform.position, rect) {
        let tc = mesh.indices.len() / 3;
        ui.painter().text(Pos2::new(center.x, center.y - 30.0), Align2::CENTER_CENTER,
                          &format!("{}K tris", tc / 1000), FontId::proportional(11.0), Color32::YELLOW);
    }
}

fn render_mesh(ui: &Ui, app: &crate::EditorApp, mesh: &crate::mesh::Mesh, material: &crate::material::Material,
               transform: &crate::math::Transform, wireframe: bool, selected: bool, rect: Rect, double_sided: bool) {
    let light_dir = Vec3::new(-0.5, -1.0, -0.5).normalize();
    let bc = if selected { Color32::from_rgb(255, 200, 100) } else {
        Color32::from_rgb((material.color[0]*255.0) as u8, (material.color[1]*255.0) as u8, (material.color[2]*255.0) as u8)
    };
    let tc = mesh.indices.len() / 3;
    let step = if tc > 50000 { 10 } else if tc > 10000 { 3 } else { 1 };
    for i in (0..tc).step_by(step) {
        let idx = i * 3;
        if idx + 2 >= mesh.indices.len() { continue; }
        let i0 = mesh.indices[idx] as usize; let i1 = mesh.indices[idx+1] as usize; let i2 = mesh.indices[idx+2] as usize;
        if i0 >= mesh.vertices.len() || i1 >= mesh.vertices.len() || i2 >= mesh.vertices.len() { continue; }
        let v0 = transform.transform_point(mesh.vertices[i0]);
        let v1 = transform.transform_point(mesh.vertices[i1]);
        let v2 = transform.transform_point(mesh.vertices[i2]);
        let e1 = v1 - v0; let e2 = v2 - v0;
        let fnorm = e1.cross(e2); let fl = fnorm.length();
        if fl < 0.0001 { continue; }
        let fnorm = fnorm * (1.0 / fl);
        if !double_sided { let vd = (app.camera_position - v0).normalize(); if fnorm.dot(vd) <= 0.0 { continue; } }
        if let (Some(p0), Some(p1), Some(p2)) = (app.world_to_screen(v0, rect), app.world_to_screen(v1, rect), app.world_to_screen(v2, rect)) {
            if wireframe {
                let ec = if selected { Color32::from_rgb(255, 220, 150) } else { Color32::WHITE };
                ui.painter().line_segment([p0, p1], (1.0, ec)); ui.painter().line_segment([p1, p2], (1.0, ec)); ui.painter().line_segment([p2, p0], (1.0, ec));
            } else {
                let br = (fnorm.dot(light_dir).max(0.0) * 0.7 + 0.3).clamp(0.0, 1.0);
                let col = Color32::from_rgb((bc.r() as f32*br) as u8, (bc.g() as f32*br) as u8, (bc.b() as f32*br) as u8);
                ui.painter().add(egui::Shape::convex_polygon(vec![p0, p1, p2], col, (0.0, Color32::TRANSPARENT)));
            }
        }
    }
}