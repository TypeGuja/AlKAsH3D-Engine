use egui::*;
use uuid::Uuid;

/// ДОБАВЛЕНО (иерархия объектов — parent/child, как в Unity/Unreal):
/// панель теперь рисует настоящее дерево (`Scene::children_of`) с отступами
/// по глубине и поддерживает drag-and-drop реродительствование через
/// встроенный DnD egui (`Ui::dnd_drag_source`/`dnd_drop_zone`) — перетащить
/// строку на другую строку делает первый объект ребёнком второго; есть
/// отдельная зона "⌂ Scene Root" вверху панели, чтобы вынести объект
/// обратно на верхний уровень. Поиск (`search_filter`) сознательно
/// показывает ПЛОСКИЙ отфильтрованный список (как было раньше) — в дереве
/// непонятно, что делать с объектом, чей родитель не проходит фильтр, а
/// плоский список с этой проблемой не сталкивается вообще.
pub fn render_hierarchy(ctx: &egui::Context, app: &mut crate::EditorApp) {
    if !app.show_hierarchy { return; }

    egui::SidePanel::left("hierarchy")
        .default_width(250.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.heading("📁 Hierarchy");
            ui.separator();

            let mut to_select: Option<Uuid> = None;
            let mut to_toggle: Option<Uuid> = None;
            let mut reparent: Option<(Uuid, Option<Uuid>)> = None;

            if !app.search_filter.is_empty() {
                let filter = app.search_filter.to_lowercase();
                let mut matches: Vec<(Uuid, String, bool, bool)> = app
                    .scene
                    .objects
                    .iter()
                    .filter(|(_, o)| o.name.to_lowercase().contains(&filter))
                    .map(|(&id, o)| (id, o.name.clone(), o.visible, app.scene.selected_ids.contains(&id)))
                    .collect();
                matches.sort_by(|a, b| a.1.cmp(&b.1));

                for (id, name, vis, sel) in matches {
                    ui.horizontal(|ui| {
                        if ui.selectable_label(false, if vis { "👁" } else { "👁‍🗨" }).clicked() {
                            to_toggle = Some(id);
                        }
                        if ui.selectable_label(sel, &name).clicked() {
                            to_select = Some(id);
                        }
                    });
                }
            } else {
                // Зона сброса на верхний уровень — перетащить сюда объект,
                // чтобы убрать его из-под текущего родителя.
                let (_, dropped_to_root) = ui.dnd_drop_zone::<Uuid, ()>(
                    egui::Frame::default().inner_margin(4.0),
                    |ui| {
                        ui.label(RichText::new("⌂ Scene Root (drop here to un-parent)").weak());
                    },
                );
                if let Some(dragged) = dropped_to_root {
                    reparent = Some((*dragged, None));
                }
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    for id in app.scene.children_of(None) {
                        render_node(ui, &app.scene, id, 0, &mut to_select, &mut to_toggle, &mut reparent);
                    }
                });
            }

            if let Some(id) = to_select {
                let add = ctx.input(|i| i.modifiers.shift);
                app.scene.select(id, add);
            }
            if let Some(id) = to_toggle {
                if let Some(obj) = app.scene.get_object_mut(id) {
                    obj.visible = !obj.visible;
                }
            }
            if let Some((child, new_parent)) = reparent {
                if let Err(e) = app.scene.set_parent(child, new_parent) {
                    app.log(&format!("⚠️ Не удалось перенести объект в иерархии: {}", e), Color32::YELLOW);
                }
            }
        });
}

fn render_node(
    ui: &mut Ui,
    scene: &crate::scene::Scene,
    id: Uuid,
    depth: u32,
    to_select: &mut Option<Uuid>,
    to_toggle: &mut Option<Uuid>,
    reparent: &mut Option<(Uuid, Option<Uuid>)>,
) {
    let Some(obj) = scene.get_object(id) else { return; };
    let name = obj.name.clone();
    let vis = obj.visible;
    let sel = scene.selected_ids.contains(&id);
    let children = scene.children_of(Some(id));

    let drag_id = egui::Id::new(("hierarchy_row", id));

    let (_zone_response, dropped_here) = ui.dnd_drop_zone::<Uuid, ()>(egui::Frame::default(), |ui| {
        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 16.0);

            if ui.selectable_label(false, if vis { "👁" } else { "👁‍🗨" }).clicked() {
                *to_toggle = Some(id);
            }

            ui.dnd_drag_source(drag_id, id, |ui| {
                let icon = match &obj.object_type {
                    crate::scene::ObjectType::Mesh(_) => "📦",
                    crate::scene::ObjectType::Light(_) => "💡",
                    crate::scene::ObjectType::Camera(_) => "🎥",
                    crate::scene::ObjectType::ParticleSystem(_) => "✨",
                    crate::scene::ObjectType::AudioSource(_) => "🔊",
                    crate::scene::ObjectType::ScriptedEntity(_) => "📜",
                    crate::scene::ObjectType::SpawnPoint => "🚩",
                    crate::scene::ObjectType::Empty => "📍",
                };
                if ui.selectable_label(sel, format!("{} {}", icon, name)).clicked() {
                    *to_select = Some(id);
                }
            });
        });
    });

    if let Some(dragged) = dropped_here {
        if *dragged != id {
            *reparent = Some((*dragged, Some(id)));
        }
    }

    for child_id in children {
        render_node(ui, scene, child_id, depth + 1, to_select, to_toggle, reparent);
    }
}
