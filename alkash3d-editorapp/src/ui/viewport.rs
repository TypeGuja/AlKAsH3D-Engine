// src/ui/viewport.rs - ПОЛНОСТЬЮ ИСПРАВЛЕННЫЙ
use egui::*;
use crate::math::Vec3;
use crate::scene::ObjectType;

pub fn render_viewport(ui: &mut Ui, app: &mut crate::EditorApp) {
    let rect = ui.available_rect_before_wrap();
    app.viewport_rect = rect;
    let bg = Color32::from_rgb((app.scene.ambient_color[0]*255.0) as u8, (app.scene.ambient_color[1]*255.0) as u8, (app.scene.ambient_color[2]*255.0) as u8);
    ui.painter().rect_filled(rect, 0.0, bg);
    if app.scene.grid_enabled { let c = Color32::from_rgb(60,60,70); for i in -20..=20 { for j in -20..=20 { let x = i as f32; let z = j as f32; if let (Some(p1),Some(p2)) = (app.world_to_screen(Vec3::new(x,0.0,z),rect), app.world_to_screen(Vec3::new(x+1.0,0.0,z),rect)) { ui.painter().line_segment([p1,p2],(1.0,c)); } if let (Some(p1),Some(p3)) = (app.world_to_screen(Vec3::new(x,0.0,z),rect), app.world_to_screen(Vec3::new(x,0.0,z+1.0),rect)) { ui.painter().line_segment([p1,p3],(1.0,c)); } } } }
    let objects: Vec<_> = app.scene.objects.values().collect();
    for obj in &objects {
        if !obj.visible { continue; }
        let world = app.scene.get_world_transform(obj.id);
        let selected = app.scene.selected_ids.contains(&obj.id);
        match &obj.object_type {
            ObjectType::Mesh(m) => { if m.solid || m.wireframe { render_mesh(ui, app, &m.mesh, &m.material, &world, m.wireframe, selected, rect, m.double_sided); } }
            ObjectType::Light(l) => { if let Some(pos) = app.world_to_screen(world.position, rect) { let col = Color32::from_rgb((l.color[0]*255.0) as u8, (l.color[1]*255.0) as u8, (l.color[2]*255.0) as u8); ui.painter().circle(pos, 15.0, col, (2.0, Color32::WHITE)); } }
            _ => {}
        }
        if let Some(pos) = app.world_to_screen(world.position + Vec3::UP * 1.0, rect) { ui.painter().text(pos, Align2::CENTER_CENTER, &obj.name, FontId::proportional(10.0), if selected { Color32::WHITE } else { Color32::LIGHT_GRAY }); }
    }
}

fn render_mesh(ui: &Ui, app: &crate::EditorApp, mesh: &crate::mesh::Mesh, material: &crate::material::Material, transform: &crate::math::Transform, wireframe: bool, selected: bool, rect: Rect, double_sided: bool) {
    let light_dir = Vec3::new(-0.5, -1.0, -0.5).normalize();
    let base_color = if selected { Color32::from_rgb(255, 200, 100) } else { Color32::from_rgb((material.color[0]*255.0) as u8, (material.color[1]*255.0) as u8, (material.color[2]*255.0) as u8) };
    let tc = mesh.indices.len() / 3;
    for i in 0..tc {
        let idx = i*3; if idx+2 >= mesh.indices.len() { continue; }
        let (i0,i1,i2) = (mesh.indices[idx] as usize, mesh.indices[idx+1] as usize, mesh.indices[idx+2] as usize);
        if i0 >= mesh.vertices.len() || i1 >= mesh.vertices.len() || i2 >= mesh.vertices.len() { continue; }
        let v0 = Vec3::new(mesh.vertices[i0].x*transform.scale.x+transform.position.x, mesh.vertices[i0].y*transform.scale.y+transform.position.y, mesh.vertices[i0].z*transform.scale.z+transform.position.z);
        let v1 = Vec3::new(mesh.vertices[i1].x*transform.scale.x+transform.position.x, mesh.vertices[i1].y*transform.scale.y+transform.position.y, mesh.vertices[i1].z*transform.scale.z+transform.position.z);
        let v2 = Vec3::new(mesh.vertices[i2].x*transform.scale.x+transform.position.x, mesh.vertices[i2].y*transform.scale.y+transform.position.y, mesh.vertices[i2].z*transform.scale.z+transform.position.z);
        let n = (v1-v0).cross(v2-v0); let len = n.length(); if len < 0.0001 { continue; } let n = n*(1.0/len);
        if !double_sided && n.dot((app.camera_position-v0).normalize()) < 0.0 { continue; }
        if let (Some(p0),Some(p1),Some(p2)) = (app.world_to_screen(v0,rect), app.world_to_screen(v1,rect), app.world_to_screen(v2,rect)) {
            if wireframe { let ec = if selected { Color32::from_rgb(255,220,150) } else { Color32::WHITE }; ui.painter().line_segment([p0,p1],(1.0,ec)); ui.painter().line_segment([p1,p2],(1.0,ec)); ui.painter().line_segment([p2,p0],(1.0,ec)); }
            else { let br = n.dot(light_dir).max(0.0)*0.7+0.3; let col = Color32::from_rgb((base_color.r() as f32*br) as u8, (base_color.g() as f32*br) as u8, (base_color.b() as f32*br) as u8); ui.painter().add(egui::Shape::convex_polygon(vec![p0,p1,p2], col, (1.0,col))); }
        }
    }
}