//! Спавн `.alasm`-сборок (машина/двигатель/коробка передач — разбираемые
//! на физически реальные детали, см. `alasm_format.rs`) в РЕАЛЬНЫЕ
//! физические тела + joints (`alkash3d-inertial`) + видимые ECS-сущности.
//!
//! Отдельный подмодуль `engine`, а не расширение `physics_bridge.rs` —
//! там мост к физике КАК ТАКОВОЙ (add_body/add_constraint/...), здесь —
//! конкретная прикладная фича (конкретный файловый формат), построенная
//! НА НЕЙ. Тот же принцип разделения, что уже развёл `world_streaming.rs`
//! (стриминг мира) и `asset_loading.rs` (загрузка .altex) по разным
//! файлам, хотя оба используют один и тот же `AlkashEngine`.

use std::path::{Path, PathBuf};
use crate::alasm_format::{AlasmFile, PartRecord, NONE_ID};
use crate::plugin::{joint_type, ConstraintDesc, PhysicsBody};
use super::AlkashEngine;

/// Одна физически заспавненная деталь сборки — хендл для дальнейшей игры
/// с ней: снять деталь вручную (`AlkashEngine::remove_constraint`),
/// проверить, не оторвало ли её силой (`AlkashEngine::get_constraint`/
/// `get_broken_constraints`), показать имя в UI разборки.
#[derive(Debug, Clone)]
pub struct AssemblyPart {
    pub body_id: i32,
    pub entity: crate::scene::EntityId,
    /// `None` ТОЛЬКО у корневой детали сборки целиком (кузов/блок
    /// цилиндров — ей не за что крепиться внутри своей же сборки). У всех
    /// остальных — handle joint'а, скрепляющего её с родителем.
    pub constraint_id: Option<i32>,
    pub name: Option<String>,
}

/// Все части одной заспавненной сборки — ПЛОСКИЙ список (не дерево:
/// иерархия уже "запечена" в joints между `body_id` реального
/// физического солвера, отдельное дерево хендлов рантайму не нужно).
/// Включает части рекурсивно вложенных под-сборок (см.
/// `PartRecord::sub_assembly_path_id`) — их корень оказывается ОБЫЧНОЙ
/// записью в этом списке, неотличимой от любой другой детали.
#[derive(Debug, Clone, Default)]
pub struct AssemblyHandle {
    pub parts: Vec<AssemblyPart>,
}

impl AssemblyHandle {
    /// `body_id` корневой детали ВСЕЙ сборки (единственная запись с
    /// `constraint_id == None`) — нужен, чтобы прикрепить сборку целиком
    /// к чему-то ещё извне (например колесо — к ступице через
    /// `add_hinge_joint(hub_body, handle.root_body_id()?, ...)`).
    pub fn root_body_id(&self) -> Option<i32> {
        self.parts.iter().find(|p| p.constraint_id.is_none()).map(|p| p.body_id)
    }
}

impl AlkashEngine {
    /// Загружает `.alasm` с диска и спавнит его целиком: по одному
    /// физическому телу (`add_physics_body`) + видимой ECS-сущности
    /// (`spawn_static_mesh`) на КАЖДУЮ деталь дерева, плюс РЕАЛЬНЫЙ joint
    /// `alkash3d-inertial` между каждой не-корневой деталью и её
    /// родителем (см. `PartRecord`). `world_position` — где окажется
    /// КОРЕНЬ сборки; остальные детали размещаются относительно него по
    /// `local_position` каждой детали (см. ограничение "накопленное
    /// смещение без учёта поворота родителя" у `spawn_assembly_into`).
    ///
    /// `None`, если файл не читается ИЛИ физика не инициализирована —
    /// без `init_physics()` не может быть ни одного физического тела, а
    /// собрать "сборку" из одних мешей без физики не имело бы смысла
    /// (тогда это обычная статичная геометрия — для неё есть
    /// `spawn_static_mesh`/world streaming).
    pub fn spawn_assembly(&mut self, alasm_path: &str, world_position: [f32; 3]) -> Option<AssemblyHandle> {
        if self.physics.is_none() {
            eprintln!("[ENGINE] spawn_assembly('{}') вызван до init_physics() — сборка не заспавнена", alasm_path);
            return None;
        }
        let asm = match AlasmFile::load(alasm_path) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[ENGINE] не удалось загрузить .alasm '{}': {}", alasm_path, e);
                return None;
            }
        };
        let base_dir = Path::new(alasm_path).parent().map(Path::to_path_buf).unwrap_or_default();

        let mut handle = AssemblyHandle::default();
        self.spawn_assembly_into(&asm, &base_dir, world_position, &mut handle);
        if handle.parts.is_empty() { None } else { Some(handle) }
    }

    /// Рекурсивный спавн одной сборки (уже прочитанной в память) — общая
    /// реализация и для верхнего `spawn_assembly` (только что прочитанный
    /// файл), и для встроенных под-сборок (см. `PartRecord::
    /// sub_assembly_path_id`). Дописывает заспавненные детали в конец
    /// `out.parts`.
    ///
    /// ВАЖНО (ограничение — то же, что уже принято для joint-анкеров в
    /// `alkash3d-inertial`, см. комментарий у `anchor_a` в
    /// `constraint_c`): позиции деталей накапливаются ПРОСТЫМ сложением
    /// `local_position` без учёта поворота родителя — верно для деталей,
    /// смещённых вдоль мировых осей от родителя (типичный случай для
    /// прямоугольной геометрии машины/двигателя), но НЕ повернёт
    /// смещение вместе с `local_rotation` родителя, если тот уже
    /// повёрнут. Для сборок, где это важно, компенсировать явным
    /// пересчётом `local_position` под нужный угол при авторинге .alasm —
    /// полноценный учёт поворота (через кватернион/матрицу) — следующий
    /// шаг, сознательно не сделанный здесь ради ограниченного объёма
    /// задачи.
    ///
    /// Порядок обхода: родитель ВСЕГДА раньше своих детей (гарантируется
    /// построителем `AlasmFile::add_child_part`) — повреждённый/
    /// рукописный файл с `parent_index`, указывающим ВПЕРЁД, просто не
    /// найдёт родителя в `spawned` и такая деталь будет пропущена с
    /// предупреждением, а не запаникует и не создаст "физику в вакууме"
    /// без родителя.
    fn spawn_assembly_into(
        &mut self,
        asm: &AlasmFile,
        base_dir: &Path,
        world_position: [f32; 3],
        out: &mut AssemblyHandle,
    ) {
        let mut spawned: Vec<Option<(i32, [f32; 3])>> = vec![None; asm.parts.len()];

        for (index, part) in asm.parts.iter().enumerate() {
            let is_root = part.parent_index == -1;
            let (parent_body, parent_pos) = if is_root {
                (None, world_position)
            } else {
                match spawned.get(part.parent_index as usize).copied().flatten() {
                    Some(v) => (Some(v.0), v.1),
                    None => {
                        eprintln!(
                            "[ENGINE] WARNING: .alasm деталь #{} ссылается на ещё не заспавненного родителя #{} — деталь пропущена",
                            index, part.parent_index
                        );
                        continue;
                    }
                }
            };
            let part_world_pos = add3(parent_pos, part.local_position);

            if part.sub_assembly_path_id != NONE_ID {
                // Встроенная под-сборка (см. шапку файла) — рекурсивно
                // грузим и спавним ЕЁ ЦЕЛИКОМ БЕЗ родителя (как если бы
                // она была верхнеуровневой), а затем создаём joint между
                // текущим родителем и ЕЁ КОРНЕМ — ТЕМИ ЖЕ joint-полями
                // ЭТОЙ записи (`part.joint_type`/`anchor_a`/...), что и у
                // обычной детали ниже. Так joint "как машина держит
                // двигатель" описывается ровно там, где ему место — в
                // ссылающемся файле, а не в самом файле двигателя (у
                // которого своя корневая деталь ни к кому не крепится,
                // когда он загружен САМ ПО СЕБЕ на верстаке).
                let Some(sub_path) = asm.get_string(part.sub_assembly_path_id) else {
                    eprintln!("[ENGINE] WARNING: .alasm деталь #{} — sub_assembly_path_id вне таблицы строк, деталь пропущена", index);
                    continue;
                };
                let resolved: PathBuf = base_dir.join(sub_path);
                let sub_asm = match AlasmFile::load(&resolved.to_string_lossy()) {
                    Ok(a) => a,
                    Err(e) => {
                        eprintln!(
                            "[ENGINE] WARNING: не удалось загрузить встроенную под-сборку '{}' (.alasm деталь #{}): {} — деталь пропущена",
                            resolved.display(), index, e
                        );
                        continue;
                    }
                };
                let sub_base_dir = resolved.parent().map(Path::to_path_buf).unwrap_or_default();

                let before_len = out.parts.len();
                self.spawn_assembly_into(&sub_asm, &sub_base_dir, part_world_pos, out);
                let Some(sub_root_body_id) = out.parts.get(before_len).map(|p| p.body_id) else {
                    eprintln!("[ENGINE] WARNING: встроенная под-сборка '{}' не заспавнила ни одной детали", resolved.display());
                    continue;
                };

                let constraint_id = parent_body.and_then(|parent_body_id| {
                    self.create_assembly_joint(part, parent_body_id, sub_root_body_id)
                });
                // Рекурсивный вызов уже добавил корень под-сборки в
                // `out.parts` с `constraint_id = None` (на тот момент он
                // сам не знал о СВОЁМ внешнем родителе) — обновляем
                // постфактум, а не пушим вторую запись на тот же body_id.
                if let Some(entry) = out.parts.get_mut(before_len) {
                    entry.constraint_id = constraint_id;
                }

                spawned[index] = Some((sub_root_body_id, part_world_pos));
                continue;
            }

            let mesh_index = match asm.get_string(part.mesh_path_id) {
                Some(path) => match self.load_object_mesh_sync(path).first().copied() {
                    Some(i) => i,
                    None => self.add_cube(0.3),
                },
                None => self.add_cube(0.3),
            };

            let body = PhysicsBody {
                position: part_world_pos,
                velocity: [0.0; 3],
                acceleration: [0.0; 3],
                angular_velocity: [0.0; 3],
                angular_acceleration: [0.0; 3],
                mass: part.mass,
                inv_mass: if part.mass > 0.0 { 1.0 / part.mass } else { 0.0 },
                restitution: part.restitution,
                friction: part.friction,
                linear_damping: 0.02,
                angular_damping: 0.1,
                is_static: if part.mass <= 0.0 { 1 } else { 0 },
                is_asleep: 0,
                orientation: [0.0, 0.0, 0.0, 1.0],
                // ИСПРАВЛЕНО (E0063, тот же паттерн, что у
                // `add_sphere_body`/`spawn_physics_car` в physics_bridge.rs):
                // .alasm-формат не хранит явный радиус детали, зато уже
                // загруженный меш (`mesh_index`) знает свой
                // `bounding_radius` (та же величина, что использует
                // frustum-каллинг в render_frame.rs) — деталь спавнится со
                // scale [1,1,1] (см. `spawn_static_mesh` ниже), поэтому
                // bounding_radius меша БЕЗ поправки на масштаб — это и есть
                // физический радиус детали в мировых единицах.
                radius: self.meshes[mesh_index].bounding_radius,
                // ИСПРАВЛЕНО (E0063 — box-коллайдер кузова машины добавил
                // два новых поля): детали `.alasm`-сборки — по-прежнему
                // сферы (см. комментарий у `radius` выше), `half_extents`
                // для них не используется.
                shape_type: crate::plugin::shape_type::SPHERE,
                half_extents: [0.0; 3],
            };
            let Some(body_id) = self.add_physics_body(body) else {
                eprintln!("[ENGINE] WARNING: .alasm деталь #{} — add_physics_body отказал (лимит max_bodies?), деталь пропущена", index);
                continue;
            };

            let entity = self.spawn_static_mesh(mesh_index, part_world_pos, part.local_rotation, [1.0, 1.0, 1.0]);
            self.physics_links.push((body_id, entity));

            let constraint_id = parent_body.and_then(|parent_body_id| {
                self.create_assembly_joint(part, parent_body_id, body_id)
            });

            spawned[index] = Some((body_id, part_world_pos));
            out.parts.push(AssemblyPart {
                body_id,
                entity,
                constraint_id,
                name: asm.get_string(part.name_id).map(str::to_string),
            });
        }
    }

    /// Общая точка создания joint'а из полей `PartRecord` — используется
    /// ОДИНАКОВО что для обычной детали, что для присоединения корня
    /// встроенной под-сборки к внешнему родителю (см. вызовы выше),
    /// чтобы поведение "какой тип соединения/порог разрушения" не
    /// расходилось между двумя путями.
    fn create_assembly_joint(&mut self, part: &PartRecord, parent_body_id: i32, body_id: i32) -> Option<i32> {
        match part.joint_type {
            joint_type::HINGE => self.add_hinge_joint(
                parent_body_id, body_id, part.anchor_a, part.anchor_b, part.axis,
                part.break_impulse_linear, part.break_impulse_angular,
            ),
            joint_type::BALL => self.add_ball_joint(
                parent_body_id, body_id, part.anchor_a, part.anchor_b, part.break_impulse_linear,
            ),
            joint_type::SLIDER => self.add_constraint(&ConstraintDesc {
                body_a: parent_body_id,
                body_b: body_id,
                joint_type: joint_type::SLIDER,
                anchor_a: part.anchor_a,
                anchor_b: part.anchor_b,
                axis_a: part.axis,
                axis_b: part.axis,
                break_impulse_linear: part.break_impulse_linear,
                break_impulse_angular: part.break_impulse_angular,
                ..Default::default()
            }),
            // FIXED и любое нераспознанное значение — самый частый случай
            // для крепежа (болты/сварка), безопасный дефолт.
            _ => self.add_fixed_joint(
                parent_body_id, body_id, part.anchor_a, part.anchor_b,
                part.break_impulse_linear, part.break_impulse_angular,
            ),
        }
    }
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
