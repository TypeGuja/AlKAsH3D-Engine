// src/ui/uv_viewer.rs
//
// ДОБАВЛЕНО (просмотр UV-развёртки — по прямому запросу пользователя):
// показывает `mesh.uv`, посчитанные `Mesh::recalculate_uv()` (см. её
// комментарий про планарную проекцию по доминирующей оси нормали и
// известное ограничение на стыках граней), как обычный 2D wireframe в
// UV-пространстве (0..1) — каждый треугольник меша рисуется линиями между
// UV его трёх вершин, без текстуры и заливки. Это инструмент "посмотреть,
// как легла развёртка", а не полноценный UV-эдитор — переносить/сшивать
// острова тут нельзя, только смотреть.
use egui::*;
use crate::scene::ObjectType;

pub fn render_uv_viewer(ctx: &egui::Context, app: &mut crate::EditorApp) {
    if !app.uv_viewer.open {
        return;
    }

    // Клонируем нужные данные меша ДО открытия окна — `app` иначе был бы
    // одновременно заимствован (для поиска объекта) и передан в замыкание
    // окна ниже, а egui::Window::show требует `&mut app` только внутри
    // замыкания, не снаружи.
    let mesh_info = app.uv_viewer.target
        .and_then(|id| app.scene.get_object(id))
        .and_then(|obj| match &obj.object_type {
            ObjectType::Mesh(m) => Some((obj.name.clone(), m.mesh.clone())),
            _ => None,
        });

    let mut open = app.uv_viewer.open;
    egui::Window::new("🗺 UV Unwrap Viewer")
        .open(&mut open)
        .default_size([420.0, 460.0])
        .show(ctx, |ui| {
            match &mesh_info {
                Some((name, mesh)) => {
                    ui.label(format!("Object: {}", name));
                    ui.label(format!("UV coords: {}   Triangles: {}", mesh.uv.len(), mesh.indices.len() / 3));
                    ui.separator();

                    if mesh.uv.len() != mesh.vertices.len() {
                        ui.colored_label(
                            Color32::YELLOW,
                            "⚠️ UV не рассчитаны для этого меша (recalculate_uv() ещё не вызывался или устарел).",
                        );
                        return;
                    }

                    let avail = ui.available_size();
                    let side = avail.x.min(avail.y).max(50.0);
                    let (rect, _resp) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
                    let painter = ui.painter();

                    painter.rect_filled(rect, 0.0, Color32::from_gray(30));

                    // Сетка 0..1 каждые 0.1 — просто ориентир масштаба, не сама развёртка.
                    for i in 0..=10 {
                        let t = i as f32 / 10.0;
                        let x = rect.left() + t * rect.width();
                        let y = rect.top() + t * rect.height();
                        let grid_color = if i == 0 || i == 10 { Color32::from_gray(120) } else { Color32::from_gray(55) };
                        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, grid_color));
                        painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, grid_color));
                    }

                    let to_screen = |uv: [f32; 2]| -> Pos2 {
                        pos2(
                            rect.left() + uv[0].clamp(0.0, 1.0) * rect.width(),
                            rect.bottom() - uv[1].clamp(0.0, 1.0) * rect.height(),
                        )
                    };

                    let stroke = Stroke::new(1.0, Color32::from_rgb(90, 200, 255));
                    for tri in mesh.indices.chunks_exact(3) {
                        let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
                        if a >= mesh.uv.len() || b >= mesh.uv.len() || c >= mesh.uv.len() {
                            continue;
                        }
                        let (pa, pb, pc) = (to_screen(mesh.uv[a]), to_screen(mesh.uv[b]), to_screen(mesh.uv[c]));
                        painter.line_segment([pa, pb], stroke);
                        painter.line_segment([pb, pc], stroke);
                        painter.line_segment([pc, pa], stroke);
                    }
                }
                None => {
                    ui.label("Нет мешей для отображения.");
                    ui.weak("Откройте развёртку через Inspector → Mesh → 🗺 View UV Unwrap...");
                }
            }
        });
    app.uv_viewer.open = open;
}
