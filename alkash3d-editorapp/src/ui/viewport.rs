use egui::*;
use crate::math::Vec3;
use crate::scene::ObjectType;

pub fn render_viewport(ui: &mut Ui, app: &mut crate::EditorApp) {
    let rect = ui.available_rect_before_wrap();
    app.viewport_rect = rect;

    let bg = Color32::from_rgb(
        (app.scene.ambient_color[0] * 255.0) as u8,
        (app.scene.ambient_color[1] * 255.0) as u8,
        (app.scene.ambient_color[2] * 255.0) as u8
    );
    ui.painter().rect_filled(rect, 0.0, bg);

    if app.scene.grid_enabled {
        let gc = Color32::from_rgb(60, 60, 70);
        for i in -20..=20 {
            for j in -20..=20 {
                let x = i as f32; let z = j as f32;
                if let (Some(p1), Some(p2)) = (app.world_to_screen(Vec3::new(x, 0.0, z), rect), app.world_to_screen(Vec3::new(x + 1.0, 0.0, z), rect)) {
                    ui.painter().line_segment([p1, p2], (1.0, gc));
                }
            }
        }
    }

    for obj in app.scene.objects.values() {
        if !obj.visible { continue; }
        let world = app.scene.get_world_transform(obj.id);
        let selected = app.scene.selected_ids.contains(&obj.id);

        match &obj.object_type {
            ObjectType::Mesh(m) => {
                if m.solid || m.wireframe {
                    let tris = m.mesh.indices.len() / 3;
                    if tris <= app.cpu_render_limit {
                        render_mesh(ui, &m.mesh, &world, selected, rect, app);
                    }
                    render_bounding_box(ui, &m.mesh, &world, selected, rect, app);
                }
            }
            ObjectType::Light(l) => {
                if let Some(pos) = app.world_to_screen(world.position, rect) {
                    let col = Color32::from_rgb((l.color[0]*255.0) as u8, (l.color[1]*255.0) as u8, (l.color[2]*255.0) as u8);
                    ui.painter().circle_filled(pos, 10.0, col);
                }
            }
            _ => {}
        }

        if let Some(pos) = app.world_to_screen(world.position + Vec3::new(0.0, 1.0, 0.0), rect) {
            ui.painter().text(pos, Align2::CENTER_CENTER, &obj.name, FontId::proportional(10.0), if selected { Color32::WHITE } else { Color32::LIGHT_GRAY });
        }
    }
}

fn render_mesh(ui: &Ui, mesh: &crate::mesh::Mesh, transform: &crate::math::Transform, selected: bool, rect: Rect, app: &crate::EditorApp) {
    let color = if selected { Color32::from_rgb(255, 200, 100) } else { Color32::from_rgb(180, 180, 200) };
    let tc = mesh.indices.len() / 3;
    let step = if tc > 1000 { 2 } else { 1 };
    for i in (0..tc).step_by(step) {
        let idx = i * 3;
        if idx + 2 >= mesh.indices.len() { continue; }
        let i0 = mesh.indices[idx] as usize;
        let i1 = mesh.indices[idx + 1] as usize;
        let i2 = mesh.indices[idx + 2] as usize;
        if i0 >= mesh.vertices.len() || i1 >= mesh.vertices.len() || i2 >= mesh.vertices.len() { continue; }
        let v0 = transform.transform_point(mesh.vertices[i0]);
        let v1 = transform.transform_point(mesh.vertices[i1]);
        let v2 = transform.transform_point(mesh.vertices[i2]);
        if let (Some(p0), Some(p1), Some(p2)) = (app.world_to_screen(v0, rect), app.world_to_screen(v1, rect), app.world_to_screen(v2, rect)) {
            ui.painter().line_segment([p0, p1], (1.0, color));
            ui.painter().line_segment([p1, p2], (1.0, color));
            ui.painter().line_segment([p2, p0], (1.0, color));
        }
    }
}

fn render_bounding_box(ui: &Ui, mesh: &crate::mesh::Mesh, transform: &crate::math::Transform, selected: bool, rect: Rect, app: &crate::EditorApp) {
    let (min, max) = mesh.bounds;
    let corners = [
        Vec3::new(min.x, min.y, min.z), Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, max.y, min.z), Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z), Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, max.z), Vec3::new(min.x, max.y, max.z),
    ];
    let transformed: Vec<Pos2> = corners.iter().filter_map(|c| app.world_to_screen(transform.transform_point(*c), rect)).collect();
    if transformed.len() < 8 { return; }
    let color = if selected { Color32::from_rgb(255, 200, 100) } else { Color32::from_rgb(255, 255, 0) };
    let edges = [(0,1),(1,2),(2,3),(3,0),(4,5),(5,6),(6,7),(7,4),(0,4),(1,5),(2,6),(3,7)];
    for &(a, b) in &edges { ui.painter().line_segment([transformed[a], transformed[b]], (1.5, color)); }
    if let Some(c) = app.world_to_screen(transform.position, rect) {
        ui.painter().text(Pos2::new(c.x, c.y - 30.0), Align2::CENTER_CENTER, &format!("{}K tris", mesh.indices.len() / 3000), FontId::proportional(11.0), Color32::YELLOW);
    }
}