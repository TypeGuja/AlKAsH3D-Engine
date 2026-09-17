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

            // ИСПРАВЛЕНО (по прямому запросу пользователя: "сделай меню
            // прокручиваемым, а то ассеты света стакаются друг на друга и
            // расширяют его") — раньше ScrollArea оборачивала ТОЛЬКО дерево
            // в ветке "без поиска" (см. историю файла); отфильтрованный по
            // поиску плоский список рисовался БЕЗ неё и рос на всю высоту
            // содержимого, раздувая панель вместо того, чтобы промотать
            // список. Теперь ScrollArea одна на весь контент под шапкой —
            // и группы, и дерево/поиск скроллятся вместе.
            //
            // ИСПРАВЛЕНО (по прямому запросу пользователя: глубокая
            // вложенность/длинные имена "съедали место" по горизонтали
            // вместо прокрутки) — ScrollArea::vertical() держит
            // auto_shrink включённым по оси X, поэтому при
            // scroll_enabled=[false, true] область просто расширяется
            // под самый широкий ряд (см. egui::ScrollArea::show, ветка
            // `(false, true) => content_size[d]`) — вложенные объекты
            // (add_space(depth * 16.0) в render_node) или длинные имена
            // раздувают панель вбок вместо горизонтальной прокрутки.
            // ScrollArea::both() включает скролл и по X — лишняя ширина
            // уходит в горизонтальный скроллбар, а не в раздувание панели.
            // ИСПРАВЛЕНО (по прямому запросу пользователя: "давай" — после
            // фикса O(N²) в `children_of` панель стала быстрой АЛГОРИТМИЧЕСКИ,
            // но всё ещё строит РЕАЛЬНЫЕ egui-виджеты (dnd_drop_zone +
            // horizontal + 2 selectable_label + drag source на строку) для
            // КАЖДОГО объекта сцены, даже если он давно прокручен за пределы
            // видимой области панели — сама постройка/layout/hit-test тысяч
            // виджетов, которые никто не видит, всё ещё стоит времени каждый
            // кадр. Раньше здесь стояла ОДНА `ScrollArea::both()` на группы+
            // дерево вместе (см. комментарий у неё — именно так чинили баг
            // "два скролла/раздувание панели"), поэтому просто заменить её
            // на `show_rows` (у которого своя, ОТДЕЛЬНАЯ область скролла)
            // вернуло бы тот самый бага назад. Вместо этого — ручное
            // отсечение НЕВИДИМЫХ строк ВНУТРИ той же единой ScrollArea:
            // строки вне текущего `ui.clip_rect()` (видимого окна скролла)
            // не рисуются вообще, а просто резервируют то же место
            // (`ui.add_space(ROW_HEIGHT)`), чтобы скроллбар и позиции
            // остальных строк не съехали.
            const ROW_HEIGHT: f32 = 20.0;

            egui::ScrollArea::both().show(ui, |ui| {
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

                    let clip = ui.clip_rect();
                    for (id, name, vis, sel) in matches {
                        if !row_is_visible(ui, clip, ROW_HEIGHT) {
                            ui.add_space(ROW_HEIGHT);
                            continue;
                        }
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
                    render_groups_section(ui, app, &mut to_select);

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

                    // ИСПРАВЛЕНО (по прямому запросу пользователя: "10-15
                    // фпс" на сцене из тысяч объектов, Hierarchy открыта):
                    // `render_node` раньше вызывал `scene.children_of(id)`
                    // — ПОЛНОЕ сканирование ВСЕХ объектов сцены — на КАЖДЫЙ
                    // узел, просто чтобы узнать, есть ли у него дети (для
                    // стрелочки ▶/▼). На N узлов верхнего уровня это O(N²)
                    // (N вызовов по O(N) каждый) — при N=3300 (типичный
                    // результат чанк-резки большой карты, см. `split_mesh_
                    // by_chunk`) это ~11 млн операций КАЖДЫЙ кадр, и вот
                    // где реально садится частота кадров, а не в GPU.
                    // Строим карту parent -> дети ОДИН раз за кадр (`build_
                    // children_map`, O(N log N) с учётом сортировки), а
                    // затем разворачиваем ВИДИМУЮ (с учётом свёрнутых
                    // поддеревьев) часть дерева в плоский список строк
                    // (`flatten_visible_rows`) — рекурсия по дереву дешёвая
                    // (только хэш-поиски, без виджетов), а реальные виджеты
                    // (`render_node_row`) строятся только для строк, не
                    // отсечённых `row_is_visible` ниже.
                    let children_map = build_children_map(&app.scene);
                    let mut rows = Vec::new();
                    flatten_visible_rows(ctx, &children_map, &mut rows);

                    let clip = ui.clip_rect();
                    for row in &rows {
                        if !row_is_visible(ui, clip, ROW_HEIGHT) {
                            ui.add_space(ROW_HEIGHT);
                            continue;
                        }
                        render_node_row(ui, &app.scene, row.id, row.depth, row.has_children, &mut to_select, &mut to_toggle, &mut reparent);
                    }
                }
            });

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

/// ДОБАВЛЕНО (по прямому запросу пользователя: "сделай возможность
/// создавать группы ассетов, которые можно свернуть, и чтобы они ещё
/// сохранялись"): именованные, сворачиваемые группы объектов сцены —
/// удобно для куч однотипных ассетов (например, уличных светильников),
/// которые иначе просто перечисляются подряд без какой-либо структуры.
/// Группа хранит ИМЕНА объектов, а не `Uuid` (см. assets/groups.rs про
/// почему), поэтому член группы, которого сейчас нет в сцене (переименовали,
/// удалили, ещё не импортировали), показывается блёкло с пометкой
/// "отсутствует", а не пропадает молча. Состояние (список групп, состав,
/// свёрнутость) сохраняется в файл рядом с проектом сразу при любом
/// изменении — см. `EditorApp::save_asset_groups`.
fn render_groups_section(ui: &mut Ui, app: &mut crate::EditorApp, to_select: &mut Option<Uuid>) {
    let mut changed = false;
    let mut group_to_delete: Option<usize> = None;

    ui.horizontal(|ui| {
        ui.strong("🗂 Groups");
        let has_selection = !app.scene.selected_ids.is_empty();
        if ui.add_enabled(has_selection, egui::Button::new("➕ New from selection"))
            .on_hover_text("Создать группу из выделенных сейчас объектов")
            .clicked()
        {
            let member_names: Vec<String> = app
                .scene
                .selected_ids
                .iter()
                .filter_map(|id| app.scene.get_object(*id))
                .map(|o| o.name.clone())
                .collect();
            app.asset_groups.push(crate::assets::AssetGroup {
                name: format!("Group {}", app.asset_groups.len() + 1),
                collapsed: false,
                member_names,
            });
            changed = true;
        }
    });

    for (gi, group) in app.asset_groups.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let arrow = if group.collapsed { "▶" } else { "▼" };
            if ui.small_button(arrow).clicked() {
                group.collapsed = !group.collapsed;
                changed = true;
            }
            if ui.text_edit_singleline(&mut group.name).changed() {
                changed = true;
            }
            ui.weak(format!("({})", group.member_names.len()));
            if ui.small_button("📌").on_hover_text("Добавить выделенные объекты в эту группу").clicked() {
                for id in app.scene.selected_ids.clone() {
                    if let Some(obj) = app.scene.get_object(id) {
                        if !group.member_names.contains(&obj.name) {
                            group.member_names.push(obj.name.clone());
                        }
                    }
                }
                changed = true;
            }
            if ui.small_button("🗑").on_hover_text("Удалить группу (объекты сцены не удаляются)").clicked() {
                group_to_delete = Some(gi);
            }
        });

        if !group.collapsed {
            let mut member_to_remove: Option<usize> = None;
            for (mi, member_name) in group.member_names.iter().enumerate() {
                let found = app.scene.objects.iter().find(|(_, o)| &o.name == member_name);
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    match found {
                        Some((&id, _)) => {
                            let sel = app.scene.selected_ids.contains(&id);
                            let icon = app.scene.get_object(id).map(icon_for_object).unwrap_or("📦");
                            if ui.selectable_label(sel, format!("{} {}", icon, member_name)).clicked() {
                                *to_select = Some(id);
                            }
                        }
                        None => {
                            ui.weak(format!("⚠ {} (отсутствует в сцене)", member_name));
                        }
                    }
                    if ui.small_button("✖").on_hover_text("Убрать из группы").clicked() {
                        member_to_remove = Some(mi);
                    }
                });
            }
            if let Some(mi) = member_to_remove {
                group.member_names.remove(mi);
                changed = true;
            }
        }
    }

    if let Some(gi) = group_to_delete {
        app.asset_groups.remove(gi);
        changed = true;
    }

    ui.separator();

    if changed {
        app.save_asset_groups();
    }
}

/// ДОБАВЛЕНО (по прямому запросу пользователя: "10-15 фпс" — см. подробный
/// комментарий у вызова в `render_hierarchy`): один проход по всем объектам
/// сцены вместо O(N) сканирования (`Scene::children_of`) на каждый узел
/// дерева. Ключ `None` — объекты верхнего уровня, ровно как принимает
/// `Scene::children_of(None)`. Список каждого родителя отсортирован по
/// имени тем же правилом, что и `children_of` — порядок в UI не меняется,
/// меняется только то, СКОЛЬКО РАЗ он пересчитывается за кадр.
fn build_children_map(scene: &crate::scene::Scene) -> std::collections::HashMap<Option<Uuid>, Vec<Uuid>> {
    let mut map: std::collections::HashMap<Option<Uuid>, Vec<Uuid>> = std::collections::HashMap::new();
    for (&id, obj) in &scene.objects {
        map.entry(obj.parent).or_default().push(id);
    }
    for children in map.values_mut() {
        children.sort_by(|a, b| {
            let na = scene.objects.get(a).map(|o| o.name.as_str()).unwrap_or("");
            let nb = scene.objects.get(b).map(|o| o.name.as_str()).unwrap_or("");
            na.cmp(nb).then(a.cmp(b))
        });
    }
    map
}

fn icon_for_object(obj: &crate::scene::GameObject) -> &'static str {
    match &obj.object_type {
        crate::scene::ObjectType::Mesh(_) => "📦",
        crate::scene::ObjectType::Light(_) => "💡",
        crate::scene::ObjectType::Camera(_) => "🎥",
        crate::scene::ObjectType::ParticleSystem(_) => "✨",
        crate::scene::ObjectType::AudioSource(_) => "🔊",
        crate::scene::ObjectType::ScriptedEntity(_) => "📜",
        crate::scene::ObjectType::SpawnPoint => "🚩",
        crate::scene::ObjectType::Empty => "📍",
    }
}

/// ДОБАВЛЕНО (по прямому запросу пользователя: "давай" — виртуализация
/// Hierarchy после фикса O(N²) в `children_of`): дешёвая проверка "строка
/// высотой `row_height`, которая началась бы на ТЕКУЩЕЙ позиции курсора
/// layout'а, попадает в видимое (проскроленное) окно `ScrollArea`" —
/// сравнение по Y с `ui.clip_rect()` (это и есть видимая область, а НЕ весь
/// контент скролла). Не продвигает курсор сама — вызывающий код обязан сам
/// либо отрисовать строку (курсор продвинется естественным образом), либо
/// вызвать `ui.add_space(row_height)` для пропущенной, чтобы общая высота
/// содержимого (а с ней и скроллбар) не съехала.
fn row_is_visible(ui: &Ui, clip: egui::Rect, row_height: f32) -> bool {
    let top = ui.cursor().top();
    let bottom = top + row_height;
    bottom >= clip.top() && top <= clip.bottom()
}

struct HierarchyRow {
    id: Uuid,
    depth: u32,
    has_children: bool,
}

/// Разворачивает дерево (не заходя в СВЁРНУТЫЕ узлы) в плоский список строк
/// сверху вниз — в том же порядке, в котором раньше их обходила рекурсия
/// `render_node`. Дёшево — только хэш-поиски по `children_map` и чтение уже
/// сохранённого состояния `CollapsingState` (`is_open()`, без отрисовки
/// виджетов), поэтому вызывается на ВСЕ видимые (не свёрнутые) узлы каждый
/// кадр, не только на те, что реально попадут в видимую область скролла —
/// само отсечение по видимости происходит позже, в `row_is_visible`, уже
/// при рендере конкретных строк.
fn flatten_visible_rows(
    ctx: &egui::Context,
    children_map: &std::collections::HashMap<Option<Uuid>, Vec<Uuid>>,
    rows: &mut Vec<HierarchyRow>,
) {
    fn visit(
        ctx: &egui::Context,
        children_map: &std::collections::HashMap<Option<Uuid>, Vec<Uuid>>,
        parent: Option<Uuid>,
        depth: u32,
        rows: &mut Vec<HierarchyRow>,
    ) {
        let empty: Vec<Uuid> = Vec::new();
        let siblings = children_map.get(&parent).unwrap_or(&empty);
        for &id in siblings {
            let has_children = children_map.get(&Some(id)).map(|v| !v.is_empty()).unwrap_or(false);
            rows.push(HierarchyRow { id, depth, has_children });

            if has_children {
                let is_open = egui::collapsing_header::CollapsingState::load_with_default_open(
                    ctx,
                    egui::Id::new(("hierarchy_collapse", id)),
                    true,
                )
                .is_open();
                if is_open {
                    visit(ctx, children_map, Some(id), depth + 1, rows);
                }
            }
        }
    }
    visit(ctx, children_map, None, 0, rows);
}

/// ДОБАВЛЕНО (по прямому запросу пользователя: "сделай возможность
/// свернуть такую большую цепочку, ну типо как список по нажатию") —
/// узлы с детьми получают стрелку ▶/▼ слева, сворачивающую поддерево.
/// Состояние открыто/закрыто хранится в `egui::collapsing_header::
/// CollapsingState` под собственным `Id` на объект (не в `Scene`/файле
/// проекта — это чисто UI-состояние текущей сессии эдитора, как в
/// Unity), поэтому не требует изменений в формате сохранения сцены.
///
/// ИЗМЕНЕНО (виртуализация — см. `flatten_visible_rows`/`row_is_visible`):
/// раньше называлась `render_node` и сама рекурсивно обходила и рисовала
/// детей — теперь рисует РОВНО одну строку (`has_children` приходит уже
/// готовым от `flatten_visible_rows`), а порядок обхода дерева и отсечение
/// невидимых строк — забота вызывающего кода в `render_hierarchy`.
fn render_node_row(
    ui: &mut Ui,
    scene: &crate::scene::Scene,
    id: Uuid,
    depth: u32,
    has_children: bool,
    to_select: &mut Option<Uuid>,
    to_toggle: &mut Option<Uuid>,
    reparent: &mut Option<(Uuid, Option<Uuid>)>,
) {
    let Some(obj) = scene.get_object(id) else { return; };
    let name = obj.name.clone();
    let vis = obj.visible;
    let sel = scene.selected_ids.contains(&id);

    let drag_id = egui::Id::new(("hierarchy_row", id));
    let mut collapse = egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        egui::Id::new(("hierarchy_collapse", id)),
        true,
    );
    let is_open = collapse.is_open();

    let (_zone_response, dropped_here) = ui.dnd_drop_zone::<Uuid, ()>(egui::Frame::default(), |ui| {
        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 16.0);

            if has_children {
                if ui.small_button(if is_open { "▼" } else { "▶" }).clicked() {
                    collapse.toggle(ui);
                }
            } else {
                ui.add_space(18.0);
            }

            if ui.selectable_label(false, if vis { "👁" } else { "👁‍🗨" }).clicked() {
                *to_toggle = Some(id);
            }

            // ИСПРАВЛЕНО (по прямому запросу пользователя: "не всегда можно
            // выделить нужный объект" в иерархии) — раньше сам `selectable_label`
            // с именем объекта был ВЛОЖЕН внутрь `dnd_drag_source`, то есть в
            // одном и том же прямоугольнике накладывались два виджета с разным
            // Sense: внутренний (имя) — Sense::click(), внешний (drag-зона) —
            // Sense::drag(). У egui `Sense::drag()`-виджет, в отличие от
            // `Sense::click_and_drag()`, считается "перетаскиваемым" СРАЗУ по
            // нажатию, без ожидания реального сдвига мыши (см.
            // egui::interaction::interact — ветка `is_dragged =
            // widget.sense.senses_drag()`). Из-за этого уже на следующий кадр
            // после нажатия `dnd_drag_source` переключался в режим отрисовки
            // "призрака" в слое `Order::Tooltip`, где виджеты не реагируют на
            // клик — и если отпускание кнопки мыши приходилось на этот кадр
            // (а не на самый первый кадр нажатия), клик по имени просто
            // терялся. Отсюда и нестабильность: иногда попадает в первый
            // кадр — выделение срабатывает, иногда нет — не срабатывает.
            // Фикс: drag-зона теперь — маленькая отдельная "ручка" (⠿), не
            // перекрывающая имя объекта; сам `selectable_label` с именем —
            // обычный клик без каких-либо конкурирующих Sense::drag виджетов
            // сверху, поэтому выделение срабатывает каждый раз.
            ui.dnd_drag_source(drag_id, id, |ui| {
                ui.weak("⠿");
            });

            let icon = icon_for_object(obj);
            if ui.selectable_label(sel, format!("{} {}", icon, name)).clicked() {
                *to_select = Some(id);
            }
        });
    });

    collapse.store(ui.ctx());

    if let Some(dragged) = dropped_here {
        if *dragged != id {
            *reparent = Some((*dragged, Some(id)));
        }
    }
}
