use std::collections::HashMap;
use uuid::Uuid;
use crate::math::Vec3;
use super::game_object::GameObject;

pub struct Scene {
    pub name: String,
    pub objects: HashMap<Uuid, GameObject>,
    pub selected_ids: Vec<Uuid>,
    pub ambient_color: [f32; 3],
    pub grid_enabled: bool,
    pub playing: bool,
    pub animation_time: f32,
    pub dirty: bool,
}

impl Scene {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            objects: HashMap::new(),
            selected_ids: Vec::new(),
            ambient_color: [0.2, 0.2, 0.25],
            grid_enabled: true,
            playing: false,
            animation_time: 0.0,
            dirty: false,
        }
    }

    pub fn add_object(&mut self, obj: GameObject) -> Uuid {
        let id = obj.id;
        self.objects.insert(id, obj);
        self.dirty = true;
        id
    }

    /// Удаляет объект И ВСЕХ его потомков (каскадно) — как в Unity/Unreal:
    /// удаление родителя без этого оставляло бы детей в сцене с
    /// `parent: Some(id)`, указывающим на несуществующий объект —
    /// `get_world_transform`/`children_of` ниже относятся к такому полю
    /// защитно (не паникуют), но с точки зрения пользователя "родитель
    /// исчез, а дети остались висеть непонятно где" — не то поведение,
    /// которое ожидается от удаления в иерархии.
    pub fn remove_object(&mut self, id: Uuid) -> Option<GameObject> {
        for child_id in self.children_of(Some(id)) {
            self.remove_object(child_id);
        }
        self.selected_ids.retain(|&sid| sid != id);
        self.dirty = true;
        self.objects.remove(&id)
    }

    /// Прямые дети объекта `parent` (`None` — объекты верхнего уровня),
    /// отсортированные по имени для стабильного порядка в UI (у сцены нет
    /// отдельного "порядка соседей" — см. комментарий у `GameObject::parent`).
    pub fn children_of(&self, parent: Option<Uuid>) -> Vec<Uuid> {
        let mut ids: Vec<Uuid> = self
            .objects
            .iter()
            .filter(|(_, obj)| obj.parent == parent)
            .map(|(&id, _)| id)
            .collect();
        ids.sort_by(|a, b| {
            let na = self.objects.get(a).map(|o| o.name.as_str()).unwrap_or("");
            let nb = self.objects.get(b).map(|o| o.name.as_str()).unwrap_or("");
            na.cmp(nb).then(a.cmp(b))
        });
        ids
    }

    /// Меняет родителя объекта `child` на `new_parent` (`None` — сделать
    /// объектом верхнего уровня). Отклоняет операцию (возвращает `Err`, не
    /// трогая сцену), если `new_parent` — это сам `child` либо один из его
    /// текущих потомков — иначе получился бы цикл в дереве, из-за которого
    /// `get_world_transform` ушла бы в бесконечную рекурсию.
    pub fn set_parent(&mut self, child: Uuid, new_parent: Option<Uuid>) -> Result<(), String> {
        if !self.objects.contains_key(&child) {
            return Err("Объект не найден".to_string());
        }
        if let Some(new_parent_id) = new_parent {
            if new_parent_id == child {
                return Err("Объект не может быть родителем самого себя".to_string());
            }
            if !self.objects.contains_key(&new_parent_id) {
                return Err("Новый родитель не найден".to_string());
            }
            // Идём вверх от нового родителя — если по пути встретим `child`,
            // значит `new_parent` сейчас является потомком `child`, и
            // операция создала бы цикл.
            let mut cursor = Some(new_parent_id);
            let mut depth = 0;
            while let Some(cur) = cursor {
                if cur == child {
                    return Err("Нельзя сделать объект дочерним по отношению к своему же потомку".to_string());
                }
                depth += 1;
                if depth > 256 {
                    return Err("Слишком глубокая иерархия — похоже на повреждённые данные".to_string());
                }
                cursor = self.objects.get(&cur).and_then(|o| o.parent);
            }
        }

        if let Some(obj) = self.objects.get_mut(&child) {
            obj.parent = new_parent;
        }
        self.dirty = true;
        Ok(())
    }

    pub fn get_object(&self, id: Uuid) -> Option<&GameObject> {
        self.objects.get(&id)
    }

    pub fn get_object_mut(&mut self, id: Uuid) -> Option<&mut GameObject> {
        self.dirty = true;
        self.objects.get_mut(&id)
    }

    pub fn selected_objects(&self) -> Vec<&GameObject> {
        self.selected_ids.iter()
            .filter_map(|id| self.objects.get(id))
            .collect()
    }

    pub fn select(&mut self, id: Uuid, add: bool) {
        if add {
            if !self.selected_ids.contains(&id) {
                self.selected_ids.push(id);
            }
        } else {
            self.selected_ids.clear();
            self.selected_ids.push(id);
        }
    }

    pub fn delete_selected(&mut self) {
        let ids: Vec<Uuid> = self.selected_ids.drain(..).collect();
        for id in ids {
            // remove_object каскадно удаляет и потомков — если родитель и
            // ребёнок оба были выделены, второй вызов на уже удалённом id
            // просто ничего не найдёт (HashMap::remove на отсутствующем
            // ключе — не ошибка).
            self.remove_object(id);
        }
        self.dirty = true;
    }

    /// Мировой transform объекта — композиция локальных `transform` вдоль
    /// всей цепочки `parent` (см. комментарий у поля `GameObject::parent`).
    /// Глубина рекурсии ограничена (256) как последняя защита от цикла в
    /// повреждённых данных — штатно циклов быть не может, `set_parent`
    /// их не допускает.
    pub fn get_world_transform(&self, id: Uuid) -> crate::math::Transform {
        self.world_transform_impl(id, 0)
    }

    fn world_transform_impl(&self, id: Uuid, depth: u32) -> crate::math::Transform {
        let Some(obj) = self.objects.get(&id) else {
            return crate::math::Transform::default();
        };
        match obj.parent {
            Some(parent_id) if depth < 256 && self.objects.contains_key(&parent_id) => {
                let parent_world = self.world_transform_impl(parent_id, depth + 1);
                parent_world.compose(&obj.transform)
            }
            _ => obj.transform,
        }
    }

    pub fn update(&mut self, delta_time: f32) {
        if self.playing {
            self.animation_time += delta_time;
        }

        // ДОБАВЛЕНО (частицы теперь реально симулируются, а не только
        // хранятся — см. `gpu/renderer.rs` про их отрисовку): каждый
        // ParticleSystem-объект эмитит/двигает/старит свои частицы каждый
        // кадр. `transform` синкается из локального transform объекта
        // ПЕРЕД update() (а не world-transform через `get_world_transform`)
        // — родительская иерархия для частиц пока не учитывается, чтобы не
        // тянуть сюда заимствование всей `Scene` изнутри `values_mut()`;
        // для объектов верхнего уровня (без родителя) разницы нет.
        for obj in self.objects.values_mut() {
            if let super::object_type::ObjectType::ParticleSystem(p) = &mut obj.object_type {
                if !p.enabled { continue; }
                p.system.transform = obj.transform.clone();
                p.system.update(delta_time);
            }
        }

        // ДОБАВЛЕНО (анимации теперь реально проигрываются — см. `ui/
        // inspector.rs` про авторинг keyframe'ов): играющая анимация
        // объекта КАЖДЫЙ кадр перезаписывает его `transform` целиком (а не
        // складывается с ним) — покуда `playing == false`, `transform`
        // остаётся полностью под ручным контролем (Transform-секция
        // инспектора), ровно как до появления этой фичи. Если у объекта
        // играют НЕСКОЛЬКО анимаций разом — что сама структура данных
        // (`HashMap<String, Animation>`) формально допускает — выигрывает
        // та, что идёт последней в порядке обхода HashMap (недетерминировано);
        // это осознанное упрощение: инспектор не мешает включить Play на
        // нескольких сразу, но одновременное проигрывание нескольких
        // анимаций на одном объекте — редкий, не поддерживаемый пока кейс.
        for obj in self.objects.values_mut() {
            for anim in obj.animations.values_mut() {
                anim.update(delta_time);
                if anim.playing {
                    anim.apply_to_transform(&mut obj.transform);
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{GameObject, ObjectType};

    fn empty(name: &str) -> GameObject {
        GameObject::new(name, ObjectType::Empty)
    }

    #[test]
    fn set_parent_composes_world_transform() {
        let mut scene = Scene::new("Test");
        let mut parent = empty("Parent");
        parent.transform.position = Vec3::new(10.0, 0.0, 0.0);
        let parent_id = scene.add_object(parent);

        let mut child = empty("Child");
        child.transform.position = Vec3::new(1.0, 0.0, 0.0);
        let child_id = scene.add_object(child);

        scene.set_parent(child_id, Some(parent_id)).expect("reparent should succeed");

        let world = scene.get_world_transform(child_id);
        assert!((world.position.x - 11.0).abs() < 1e-4, "expected child world x=11, got {}", world.position.x);

        // Двигаем родителя — мировая позиция ребёнка должна сдвинуться вместе с ним.
        scene.get_object_mut(parent_id).unwrap().transform.position = Vec3::new(20.0, 0.0, 0.0);
        let world2 = scene.get_world_transform(child_id);
        assert!((world2.position.x - 21.0).abs() < 1e-4);
    }

    #[test]
    fn set_parent_rejects_cycle() {
        let mut scene = Scene::new("Test");
        let a = scene.add_object(empty("A"));
        let b = scene.add_object(empty("B"));
        scene.set_parent(b, Some(a)).unwrap(); // B is child of A

        // Пытаемся сделать A ребёнком своего же потомка B — должно быть отклонено.
        assert!(scene.set_parent(a, Some(b)).is_err());
        // И самого себя.
        assert!(scene.set_parent(a, Some(a)).is_err());

        // Исходная иерархия не должна была измениться.
        assert_eq!(scene.get_object(a).unwrap().parent, None);
        assert_eq!(scene.get_object(b).unwrap().parent, Some(a));
    }

    #[test]
    fn remove_object_cascades_to_children() {
        let mut scene = Scene::new("Test");
        let parent = scene.add_object(empty("Parent"));
        let child = scene.add_object(empty("Child"));
        let grandchild = scene.add_object(empty("Grandchild"));
        scene.set_parent(child, Some(parent)).unwrap();
        scene.set_parent(grandchild, Some(child)).unwrap();

        scene.remove_object(parent);

        assert!(scene.get_object(parent).is_none());
        assert!(scene.get_object(child).is_none());
        assert!(scene.get_object(grandchild).is_none());
    }

    #[test]
    fn children_of_returns_direct_children_sorted_by_name() {
        let mut scene = Scene::new("Test");
        let parent = scene.add_object(empty("Parent"));
        let c1 = scene.add_object(empty("Zebra"));
        let c2 = scene.add_object(empty("Apple"));
        scene.set_parent(c1, Some(parent)).unwrap();
        scene.set_parent(c2, Some(parent)).unwrap();

        let children = scene.children_of(Some(parent));
        assert_eq!(children, vec![c2, c1]); // Apple before Zebra
        assert_eq!(scene.children_of(None), vec![parent]);
    }
}
