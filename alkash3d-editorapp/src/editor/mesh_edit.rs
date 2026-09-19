// src/editor/mesh_edit.rs
//
// ДОБАВЛЕНО (по прямому запросу пользователя — "сделай возможность
// редактировать фигуры, делать новые"): первый срез полноценного
// vertex/face-редактора — выделение вершин/граней, перемещение через
// gizmo, экструзия граней, удаление вершин/граней. Этот файл — ЧИСТАЯ
// геометрия (никакого egui/wgpu), операции работают прямо над
// `crate::mesh::Mesh` и множеством выбранных индексов; вся интерактивная
// часть (клик, драг gizmo, отрисовка маркеров) — в app.rs, там же, где уже
// живёт вся остальная работа с курсором/вьюпортом.
//
// ИЗВЕСТНОЕ ОГРАНИЧЕНИЕ (сознательное, для первого среза): экструзия
// поддержана ТОЛЬКО для выделения ГРАНЕЙ (Face) — у вершины самой по себе
// нет нормали грани, вдоль которой можно осмысленно выдавить новую
// геометрию, а чисто рёберная (без граней) геометрия в этом движке не
// представима (индексы всегда описывают треугольники). Экструзия одной
// вершины/ребра — отдельная, самостоятельная задача на будущее.
//
// ДРУГОЕ ОГРАНИЧЕНИЕ: новые вершины при экструзии не дублируются под
// "жёсткие" грани (каждая грань — свою копию вершины ради плоского
// шейдинга) — они просто продолжают уже существующий индекс, ровно как
// это уже делают ЗАКРУГЛЁННЫЕ примитивы этого движка (сфера/цилиндр/тор —
// см. mesh/primitives.rs, там общие вершины между соседними треугольниками
// нужны ради гладкого шейдинга). `create_cube()`, для сравнения, с правки
// "честная UV-развёртка примитивов" уже не разделяет вершины между гранями
// (у каждой грани свои 4 вершины) — но и то не ради flat-shading экструзии,
// а чтобы UV не схлопывалась в углах (см. mesh/uv.rs). Разделение вершин
// под flat shading для ВСЕХ операций редактора (не только примитивов) —
// отдельное, самостоятельное улучшение, не требуется, чтобы экструзия
// работала корректно геометрически.

use std::collections::{BTreeSet, HashMap};

use crate::math::Vec3;
use crate::mesh::Mesh;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshSelectMode {
    Vertex,
    Face,
}

impl Default for MeshSelectMode {
    fn default() -> Self {
        MeshSelectMode::Vertex
    }
}

/// Треугольник с индексом `face_idx` (0-based, "грань №N") — соответствует
/// `mesh.indices[face_idx*3 .. face_idx*3+3]`.
pub fn face_count(mesh: &Mesh) -> usize {
    mesh.indices.len() / 3
}

pub fn face_vertex_indices(mesh: &Mesh, face_idx: usize) -> Option<[usize; 3]> {
    let base = face_idx * 3;
    if base + 2 >= mesh.indices.len() {
        return None;
    }
    Some([
        mesh.indices[base] as usize,
        mesh.indices[base + 1] as usize,
        mesh.indices[base + 2] as usize,
    ])
}

/// Мировой (точнее — локальный, в пространстве самого меша) центроид грани
/// — среднее трёх её вершин. `None`, если индекс вне диапазона или
/// ссылается на несуществующую вершину (испорченный меш).
pub fn face_centroid(mesh: &Mesh, face_idx: usize) -> Option<Vec3> {
    let [a, b, c] = face_vertex_indices(mesh, face_idx)?;
    let (va, vb, vc) = (*mesh.vertices.get(a)?, *mesh.vertices.get(b)?, *mesh.vertices.get(c)?);
    Some((va + vb + vc) * (1.0 / 3.0))
}

/// Нормаль грани (не нормализованная сумма нормалей вершин, а честная
/// геометрическая нормаль ИМЕННО этого треугольника) — `None`, если индекс
/// вне диапазона или треугольник вырожден (нулевая площадь).
fn face_normal(mesh: &Mesh, face_idx: usize) -> Option<Vec3> {
    let [a, b, c] = face_vertex_indices(mesh, face_idx)?;
    let (va, vb, vc) = (*mesh.vertices.get(a)?, *mesh.vertices.get(b)?, *mesh.vertices.get(c)?);
    let n = (vb - va).cross(vc - va);
    let len = n.length();
    if len > 1e-8 { Some(n * (1.0 / len)) } else { None }
}

/// Смежность граней по общим рёбрам: `adjacency[f]` — индексы граней,
/// которые делят с гранью `f` хотя бы одно (неориентированное) ребро.
/// Ребро, встречающееся больше чем в 2 гранях (неманифолдная геометрия) —
/// пропускается: для группировки "плоской видимой поверхности" смежность
/// имеет смысл только когда у ребра ровно 2 соседа.
fn build_face_adjacency(mesh: &Mesh) -> Vec<Vec<usize>> {
    let count = face_count(mesh);
    let mut edge_to_faces: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for f in 0..count {
        let Some([a, b, c]) = face_vertex_indices(mesh, f) else { continue; };
        for &(x, y) in &[(a, b), (b, c), (c, a)] {
            let key = if x < y { (x, y) } else { (y, x) };
            edge_to_faces.entry(key).or_default().push(f);
        }
    }
    let mut adjacency = vec![Vec::new(); count];
    for faces in edge_to_faces.values() {
        if faces.len() == 2 {
            adjacency[faces[0]].push(faces[1]);
            adjacency[faces[1]].push(faces[0]);
        }
    }
    adjacency
}

/// ДОБАВЛЕНО (баг, найденный пользователем: клик по "грани" куба реально
/// выделял только ОДИН из двух треугольников её квада — перетаскивание
/// срывало половину грани, вторая половина оставалась на месте, перекашивая
/// меш): расширяет клик по ОДНОЙ грани до всей связной КОПЛАНАРНОЙ группы —
/// от `start_face` через рёбра к соседям, пока нормаль соседа совпадает с
/// нормалью старта (допуск ~2.5°, `COS_TOLERANCE`). Для плоских
/// поверхностей (куб, плоскость) это ровно то, что пользователь визуально
/// воспринимает как "одна грань" — для изогнутых (сфера, цилиндр) рост
/// естественно останавливается на первом же соседе другого наклона, там
/// каждый треугольник и остаётся своей отдельной гранью, как ожидается.
pub fn coplanar_face_group(mesh: &Mesh, start_face: usize) -> BTreeSet<usize> {
    let Some(start_normal) = face_normal(mesh, start_face) else {
        return [start_face].into_iter().collect();
    };
    let adjacency = build_face_adjacency(mesh);
    const COS_TOLERANCE: f32 = 0.999;

    let mut visited: BTreeSet<usize> = BTreeSet::new();
    let mut stack = vec![start_face];
    visited.insert(start_face);
    while let Some(f) = stack.pop() {
        let Some(neighbors) = adjacency.get(f) else { continue; };
        for &neighbor in neighbors {
            if visited.contains(&neighbor) {
                continue;
            }
            if let Some(n) = face_normal(mesh, neighbor) {
                if n.dot(start_normal) >= COS_TOLERANCE {
                    visited.insert(neighbor);
                    stack.push(neighbor);
                }
            }
        }
    }
    visited
}

/// ДОБАВЛЕНО (по прямому запросу пользователя — "сделай возможность двигать
/// [треугольники одной грани] отдельно"): треугольники, делящие ребро,
/// делят и ДВЕ вершины этого ребра — те же самые индексы. Перемещение
/// `selected_faces` без предварительного "отрыва" тянет за собой ЛЮБУЮ
/// НЕвыделенную грань, которая ссылается на те же вершины (это не баг, а
/// то, как вообще устроена geometry с общими вершинами — ИМЕННО так
/// работает обычное перемещение в любом 3D-редакторе). Чтобы подвинуть
/// `selected_faces` по-настоящему независимо от соседей, их общие с
/// НЕвыделенными гранями вершины нужно СНАЧАЛА продублировать — эта
/// функция и делает: для каждой вершины выделения, на которую ссылается
/// хотя бы одна НЕвыделенная грань, создаёт её копию (та же позиция) и
/// переключает на неё индексы ТОЛЬКО выделенных граней — исходная вершина
/// остаётся на месте, её продолжают использовать соседние грани. После
/// этого на месте разрыва образуется видимая дырка/трещина, как только
/// выделение реально сдвинут — это ожидаемый результат "раздельного"
/// перемещения, не побочный эффект.
///
/// Возвращает НОВОЕ множество индексов вершин выделения (после
/// дублирования часть индексов меняется) — вызывающий код должен заменить
/// им своё текущее выделение вершин.
pub fn detach_faces_from_neighbors(mesh: &mut Mesh, selected_faces: &BTreeSet<usize>) -> BTreeSet<usize> {
    let total = face_count(mesh);

    // Какие вершины использует хотя бы одна НЕвыделенная грань.
    let mut used_by_unselected: HashMap<usize, bool> = HashMap::new();
    for f in 0..total {
        let Some([a, b, c]) = face_vertex_indices(mesh, f) else { continue; };
        if selected_faces.contains(&f) {
            continue;
        }
        for &v in &[a, b, c] {
            used_by_unselected.insert(v, true);
        }
    }

    let mut selection_vertices: BTreeSet<usize> = BTreeSet::new();
    for &f in selected_faces {
        if let Some([a, b, c]) = face_vertex_indices(mesh, f) {
            selection_vertices.insert(a);
            selection_vertices.insert(b);
            selection_vertices.insert(c);
        }
    }

    // Дублируем ТОЛЬКО те вершины выделения, что реально общие с
    // НЕвыделенной гранью — вершины, которые использует исключительно
    // выделение, дублировать незачем (с ними и так никто не делится).
    let mut remap: HashMap<usize, usize> = HashMap::new();
    for &v in &selection_vertices {
        if used_by_unselected.get(&v).copied().unwrap_or(false) {
            let pos = mesh.vertices[v];
            let new_idx = mesh.vertices.len();
            mesh.vertices.push(pos);
            remap.insert(v, new_idx);
        }
    }

    if remap.is_empty() {
        // Нечего отрывать — выделение и так уже независимо от соседей.
        return selection_vertices;
    }

    for &f in selected_faces {
        let base = f * 3;
        if base + 2 >= mesh.indices.len() {
            continue;
        }
        for k in 0..3 {
            let old = mesh.indices[base + k] as usize;
            if let Some(&new_v) = remap.get(&old) {
                mesh.indices[base + k] = new_v as u32;
            }
        }
    }

    let mut result = BTreeSet::new();
    for &f in selected_faces {
        if let Some([a, b, c]) = face_vertex_indices(mesh, f) {
            result.insert(a);
            result.insert(b);
            result.insert(c);
        }
    }

    mesh.recalculate_normals();
    mesh.recalculate_bounds();
    mesh.recalculate_uv();
    result
}

/// Объединение индексов вершин всех граней в `selected_faces` — то, что
/// реально двигает gizmo, когда режим выделения — Face (в отличие от
/// Vertex, где gizmo двигает `selected_vertices` напрямую).
pub fn vertices_of_faces(mesh: &Mesh, selected_faces: &BTreeSet<usize>) -> BTreeSet<usize> {
    let mut out = BTreeSet::new();
    for &f in selected_faces {
        if let Some(idx) = face_vertex_indices(mesh, f) {
            out.extend(idx);
        }
    }
    out
}

/// Сдвигает выбранные вершины на `delta` — `delta` уже в ЛОКАЛЬНОМ
/// пространстве меша (без учёта transform объекта; см. вызывающий код в
/// app.rs про перевод мирового смещения gizmo в локальное).
pub fn move_vertices(mesh: &mut Mesh, selected: &BTreeSet<usize>, delta: Vec3) {
    if delta.length_squared() <= 0.0 {
        return;
    }
    for &i in selected {
        if let Some(v) = mesh.vertices.get_mut(i) {
            *v = *v + delta;
        }
    }
    mesh.recalculate_normals();
    mesh.recalculate_bounds();
    mesh.recalculate_uv();
}

/// Экструдирует выбранный "пятачок" граней: дублирует их вершины, строит
/// боковые стенки по ГРАНИЦЕ выделения (рёбра, встречающиеся среди
/// выделенных граней РОВНО один раз — рёбра, встречающиеся дважды, значит
/// обе соседние грани тоже выделены, там стенка не нужна, это "внутреннее"
/// ребро выдавливаемой крышки) и сдвигает новые вершины вдоль среднего
/// нормали выделения на `amount`. Возвращает индексы НОВЫХ (выдавленных)
/// вершин — вызывающий код обычно сразу делает их новым выделением, чтобы
/// gizmo подхватил только что созданную "крышку".
///
/// Пустое выделение или полностью вырожденные (нулевая площадь) грани —
/// не паникуют, возвращают пустое множество, ничего не меняя в меше.
pub fn extrude_faces(mesh: &mut Mesh, selected_faces: &BTreeSet<usize>, amount: f32) -> BTreeSet<usize> {
    if selected_faces.is_empty() {
        return BTreeSet::new();
    }

    let mut directed_edges: Vec<(usize, usize)> = Vec::new();
    let mut edge_count: HashMap<(usize, usize), u32> = HashMap::new();
    let mut verts_in_selection: BTreeSet<usize> = BTreeSet::new();
    let mut avg_normal = Vec3::ZERO;

    for &face_idx in selected_faces {
        let Some([a, b, c]) = face_vertex_indices(mesh, face_idx) else { continue; };
        verts_in_selection.insert(a);
        verts_in_selection.insert(b);
        verts_in_selection.insert(c);

        for &(x, y) in &[(a, b), (b, c), (c, a)] {
            directed_edges.push((x, y));
            let key = if x < y { (x, y) } else { (y, x) };
            *edge_count.entry(key).or_insert(0) += 1;
        }

        if let (Some(&va), Some(&vb), Some(&vc)) = (mesh.vertices.get(a), mesh.vertices.get(b), mesh.vertices.get(c)) {
            let n = (vb - va).cross(vc - va);
            let len = n.length();
            if len > 1e-8 {
                avg_normal = avg_normal + n * (1.0 / len);
            }
        }
    }

    if verts_in_selection.is_empty() {
        return BTreeSet::new();
    }

    let normal = if avg_normal.length() > 1e-6 { avg_normal.normalize() } else { Vec3::UP };

    // Рёбра, встречающиеся ровно один раз среди директед-рёбер выделения —
    // граница "пятачка", по ней строятся боковые стенки. Направление ИЗ
    // `directed_edges` (а не пересобранное из ключа) — важно для навивки
    // новых треугольников стенки в ту же сторону, что и остальной меш.
    let boundary_edges: Vec<(usize, usize)> = directed_edges
        .into_iter()
        .filter(|&(x, y)| {
            let key = if x < y { (x, y) } else { (y, x) };
            edge_count.get(&key).copied().unwrap_or(0) == 1
        })
        .collect();

    // Дублируем вершины выделения, сдвинутые вдоль нормали.
    let mut old_to_new: HashMap<usize, usize> = HashMap::with_capacity(verts_in_selection.len());
    for &v in &verts_in_selection {
        let Some(&pos) = mesh.vertices.get(v) else { continue; };
        let new_idx = mesh.vertices.len();
        mesh.vertices.push(pos + normal * amount);
        old_to_new.insert(v, new_idx);
    }

    // "Крышка" (сами выделенные грани) теперь ссылается на НОВЫЕ (выдвинутые)
    // вершины — визуально это и есть "поднятая" часть меша.
    for &face_idx in selected_faces {
        let base = face_idx * 3;
        if base + 2 >= mesh.indices.len() {
            continue;
        }
        for k in 0..3 {
            let old = mesh.indices[base + k] as usize;
            if let Some(&new_v) = old_to_new.get(&old) {
                mesh.indices[base + k] = new_v as u32;
            }
        }
    }

    // Боковые стенки: для директед-ребра (a -> b) границы — квад
    // (a, b, new_b, new_a), два треугольника (a,b,new_b) и (a,new_b,new_a) —
    // стандартная навивка "extrude", согласованная с направлением ребра
    // исходной (уже правильно навитой наружу) грани.
    for (a, b) in boundary_edges {
        let (Some(&new_a), Some(&new_b)) = (old_to_new.get(&a), old_to_new.get(&b)) else { continue; };
        mesh.indices.extend_from_slice(&[
            a as u32, b as u32, new_b as u32,
            a as u32, new_b as u32, new_a as u32,
        ]);
    }

    mesh.recalculate_normals();
    mesh.recalculate_bounds();
    mesh.recalculate_uv();

    old_to_new.values().copied().collect()
}

/// Удаляет выбранные ГРАНИ (треугольники) — вершины остаются на месте, даже
/// если после удаления на них не ссылается ни одна грань (простое и
/// предсказуемое поведение для первого среза; "сборка мусора" неиспользуемых
/// вершин — самостоятельное будущее улучшение, не влияет на корректность).
pub fn delete_faces(mesh: &mut Mesh, selected_faces: &BTreeSet<usize>) {
    if selected_faces.is_empty() {
        return;
    }
    let total_faces = face_count(mesh);
    let mut new_indices = Vec::with_capacity(mesh.indices.len());
    for face_idx in 0..total_faces {
        if selected_faces.contains(&face_idx) {
            continue;
        }
        let base = face_idx * 3;
        new_indices.extend_from_slice(&mesh.indices[base..base + 3]);
    }
    mesh.indices = new_indices;
    mesh.recalculate_normals();
    mesh.recalculate_bounds();
    mesh.recalculate_uv();
}

/// Удаляет выбранные ВЕРШИНЫ — сначала убирает любую грань, ссылающуюся
/// хотя бы на одну удаляемую вершину (иначе индекс остался бы висячим),
/// затем компактно переиндексирует оставшиеся вершины/грани. Порядок
/// важен: грани фильтруются ДО перестроения индексов, поэтому к моменту
/// ремаппинга ни один оставшийся индекс не может указывать на удалённую
/// вершину — `remap[...]` ниже гарантированно `Some`.
pub fn delete_vertices(mesh: &mut Mesh, selected: &BTreeSet<usize>) {
    if selected.is_empty() {
        return;
    }

    let total_faces = face_count(mesh);
    let mut kept_indices = Vec::with_capacity(mesh.indices.len());
    for face_idx in 0..total_faces {
        let Some([a, b, c]) = face_vertex_indices(mesh, face_idx) else { continue; };
        if selected.contains(&a) || selected.contains(&b) || selected.contains(&c) {
            continue;
        }
        kept_indices.extend_from_slice(&[a as u32, b as u32, c as u32]);
    }

    let old_count = mesh.vertices.len();
    let mut remap: Vec<Option<u32>> = vec![None; old_count];
    let mut new_vertices = Vec::with_capacity(old_count.saturating_sub(selected.len()));
    for i in 0..old_count {
        if selected.contains(&i) {
            continue;
        }
        remap[i] = Some(new_vertices.len() as u32);
        new_vertices.push(mesh.vertices[i]);
    }

    for idx in kept_indices.iter_mut() {
        *idx = remap[*idx as usize].expect("грань, ссылающаяся на удалённую вершину, должна была быть отфильтрована выше");
    }

    mesh.vertices = new_vertices;
    mesh.indices = kept_indices;
    mesh.recalculate_normals();
    mesh.recalculate_bounds();
    mesh.recalculate_uv();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selected(items: &[usize]) -> BTreeSet<usize> {
        items.iter().copied().collect()
    }

    #[test]
    fn coplanar_face_group_selects_both_triangles_of_a_cube_side() {
        let mesh = Mesh::create_cube();
        // Верхняя грань куба — треугольники 10 и 11 (см. mesh/primitives.rs).
        let group = coplanar_face_group(&mesh, 10);
        assert_eq!(group, selected(&[10, 11]), "клик по одному треугольнику квада должен выделить оба");
        // Симметрично: клик по ВТОРОМУ треугольнику даёт ту же группу.
        let group2 = coplanar_face_group(&mesh, 11);
        assert_eq!(group2, selected(&[10, 11]));

        // Соседняя (но НЕ компланарная — другая грань куба) не попадает в группу.
        assert!(!group.contains(&0), "back-грань куба не должна попасть в группу top-грани");
    }

    #[test]
    fn coplanar_face_group_stops_at_curved_surface() {
        let mesh = Mesh::create_sphere();
        // У сферы соседние треугольники почти никогда не строго компланарны
        // (у каждого своя нормаль) — группа должна остаться маленькой
        // (как минимум не "весь меш").
        let group = coplanar_face_group(&mesh, 0);
        assert!(group.len() < face_count(&mesh), "на сфере компланарная группа не должна поглотить весь меш");
    }

    #[test]
    fn detach_faces_from_neighbors_isolates_shared_vertices() {
        // ОБНОВЛЕНО (честная UV-развёртка примитивов): раньше этот тест
        // гонял `Mesh::create_cube()` — у куба КАЖДЫЙ угол был общим сразу
        // для 3 граней, идеальный подопытный для detach. После правки куб
        // сам по себе уже "жёсткий" (у каждой грани свои 4 вершины, см.
        // mesh/primitives.rs), то есть больше не иллюстрирует сценарий
        // "разделяемая соседями вершина" вообще — используем
        // `Mesh::create_plane()` (2 треугольника квада, честно делят между
        // собой 2 из 4 вершин) как подопытного вместо куба; сама проверяемая
        // ЛОГИКА detach не изменилась.
        let mut mesh = Mesh::create_plane();
        let vertex_count_before = mesh.vertices.len();
        // Плоскость — квад из 2 треугольников: 0=(0,1,2), 1=(2,3,0) (см.
        // mesh/primitives.rs::create_plane). Вершины 0 и 2 — общее ребро
        // диагонали, делят их ОБА треугольника; вершина 1 — только у
        // треугольника 0, вершина 3 — только у треугольника 1.
        let one_triangle = selected(&[0]);

        let new_verts = detach_faces_from_neighbors(&mut mesh, &one_triangle);

        assert_eq!(mesh.vertices.len(), vertex_count_before + 2, "у квада общее с соседом — только диагональное ребро (2 вершины)");
        // `new_verts` — ВСЕ вершины выделенной грани ПОСЛЕ remap'а (не
        // только реально продублированные) — для одного треугольника это
        // всегда 3, вне зависимости от того, сколько из них было общими с
        // соседом (см. `result` в конце `detach_faces_from_neighbors`).
        assert_eq!(new_verts.len(), 3);

        // Грань 1 (сосед по диагонали) должна остаться на СТАРЫХ вершинах —
        // её индексы вообще не должны были поменяться.
        assert_eq!(face_vertex_indices(&mesh, 1), Some([2, 3, 0]));

        // Грань 0 теперь ссылается на НОВЫЕ (продублированные) индексы там,
        // где делила вершины с соседом (0 и 2) — но сохраняет старый индекс
        // вершины 1, которую ни с кем делить и не приходилось.
        let updated = face_vertex_indices(&mesh, 0).unwrap();
        assert!(!updated.contains(&0) && !updated.contains(&2), "общие с соседом вершины должны смениться на новые копии");
        assert!(updated.contains(&1), "вершина, ни с кем не общая, не должна была дублироваться");

        // Повторный вызов на УЖЕ оторванном выделении не должен ничего
        // менять — новые вершины грани 0 теперь ничьи, кроме неё самой.
        let vertex_count_after_first = mesh.vertices.len();
        detach_faces_from_neighbors(&mut mesh, &one_triangle);
        assert_eq!(mesh.vertices.len(), vertex_count_after_first, "повторный отрыв уже независимого выделения — no-op");
    }

    #[test]
    fn move_vertices_shifts_only_selected() {
        let mut mesh = Mesh::create_cube();
        let before = mesh.vertices.clone();
        move_vertices(&mut mesh, &selected(&[0]), Vec3::new(1.0, 0.0, 0.0));
        assert!((mesh.vertices[0] - (before[0] + Vec3::new(1.0, 0.0, 0.0))).length() < 1e-6);
        for i in 1..before.len() {
            assert!((mesh.vertices[i] - before[i]).length() < 1e-6, "vertex {} moved unexpectedly", i);
        }
    }

    #[test]
    fn extrude_top_face_of_cube_adds_side_walls_and_moves_cap_outward() {
        let mut mesh = Mesh::create_cube();
        let vertex_count_before = mesh.vertices.len();
        let face_count_before = face_count(&mesh);

        // Верхняя грань куба — грани 10 и 11, последний квад из 6 (см.
        // порядок в mesh/primitives.rs::create_cube).
        let top_faces = selected(&[10, 11]);
        let new_verts = extrude_faces(&mut mesh, &top_faces, 1.0);

        assert_eq!(new_verts.len(), 4, "у верхней грани куба 4 уникальные вершины");
        assert_eq!(mesh.vertices.len(), vertex_count_before + 4);
        // 2 исходные грани "крышки" остаются (переиндексированы на новые
        // вершины) + 4 боковых стенки * 2 треугольника = 8 новых граней.
        assert_eq!(face_count(&mesh), face_count_before + 8);

        // Новые вершины подняты по Y относительно старых как минимум на amount.
        for &nv in &new_verts {
            assert!(mesh.vertices[nv].y > 0.5 + 0.9, "extruded vertex should be pushed outward: {:?}", mesh.vertices[nv]);
        }
    }

    #[test]
    fn extrude_empty_selection_is_noop() {
        let mut mesh = Mesh::create_cube();
        let before_verts = mesh.vertices.len();
        let before_faces = face_count(&mesh);
        let new_verts = extrude_faces(&mut mesh, &BTreeSet::new(), 1.0);
        assert!(new_verts.is_empty());
        assert_eq!(mesh.vertices.len(), before_verts);
        assert_eq!(face_count(&mesh), before_faces);
    }

    #[test]
    fn delete_faces_removes_only_selected_triangles() {
        let mut mesh = Mesh::create_cube();
        let before_faces = face_count(&mesh);
        delete_faces(&mut mesh, &selected(&[0]));
        assert_eq!(face_count(&mesh), before_faces - 1);
    }

    #[test]
    fn delete_vertices_removes_dependent_faces_and_reindexes() {
        let mut mesh = Mesh::create_cube();
        let before_faces = face_count(&mesh);
        let before_vertices = mesh.vertices.len();
        // ОБНОВЛЕНО (честная UV-развёртка примитивов): с "жёсткими" гранями
        // куба (см. mesh/primitives.rs) вершина 0 принадлежит только ОДНОЙ
        // грани (обоим её треугольникам), а не трём сразу, как было со
        // старой shared-vertex топологией — но сама проверяемая логика
        // (удаление вершины удаляет ровно те грани, что её используют, и
        // компактно переиндексирует остальное) от этого не меняется.
        let faces_using_v0 = (0..before_faces)
            .filter(|&f| face_vertex_indices(&mesh, f).unwrap().contains(&0))
            .count();
        assert!(faces_using_v0 > 0);

        delete_vertices(&mut mesh, &selected(&[0]));

        assert_eq!(mesh.vertices.len(), before_vertices - 1);
        assert_eq!(face_count(&mesh), before_faces - faces_using_v0);
        // Все оставшиеся индексы должны быть в новых границах.
        for &idx in &mesh.indices {
            assert!((idx as usize) < mesh.vertices.len());
        }
    }
}
