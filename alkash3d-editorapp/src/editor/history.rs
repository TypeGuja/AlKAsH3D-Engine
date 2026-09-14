use std::collections::VecDeque;
use uuid::Uuid;
use crate::math::Transform;
use crate::scene::Scene;

#[derive(Debug, Clone)]
pub enum EditorCommand {
    CreateObject { id: Uuid, object: crate::scene::GameObject },
    DeleteObject { id: Uuid, object: crate::scene::GameObject },
    ModifyTransform { id: Uuid, old_transform: Transform, new_transform: Transform },
    // ДОБАВЛЕНО (undo/redo для mesh-редактора — по прямому запросу
    // пользователя, следующий пункт плана после снап-фичи): move/extrude/
    // delete вершин и граней меняют и число вершин, и число индексов, так
    // что точечный "старое/новое значение поля" (как у ModifyTransform) не
    // подходит — проще и надёжнее хранить ПОЛНЫЙ снимок меша до и после
    // правки (Mesh уже Clone, а меши редактируемых объектов небольшие —
    // это не world-стриминг). Один такой снимок соответствует ОДНОМУ жесту
    // пользователя (весь драг перетаскивания, один вызов extrude, один
    // delete), а не каждому кадру внутри него.
    ModifyMesh { id: Uuid, old_mesh: crate::mesh::Mesh, new_mesh: crate::mesh::Mesh },
}

pub struct CommandHistory {
    undo_stack: VecDeque<EditorCommand>,
    redo_stack: VecDeque<EditorCommand>,
    max_size: usize,
}

impl CommandHistory {
    pub fn new(max_size: usize) -> Self {
        Self {
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            max_size,
        }
    }

    pub fn push(&mut self, command: EditorCommand) {
        if self.undo_stack.len() >= self.max_size {
            self.undo_stack.pop_back();
        }
        self.undo_stack.push_front(command);
        self.redo_stack.clear();
    }

    /// Возвращает id объекта, которого коснулась отменённая команда (если
    /// был), чтобы вызывающий код (см. `app.rs`) мог обновить GPU-меш/сбросить
    /// выделение в mesh-редакторе — сам `CommandHistory` ничего не знает ни о
    /// GPU, ни об Edit Mode.
    pub fn undo(&mut self, scene: &mut Scene) -> Option<Uuid> {
        if let Some(command) = self.undo_stack.pop_front() {
            let id = Self::command_object_id(&command);
            self.apply_undo(command.clone(), scene);
            self.redo_stack.push_front(command);
            id
        } else {
            None
        }
    }

    pub fn redo(&mut self, scene: &mut Scene) -> Option<Uuid> {
        if let Some(command) = self.redo_stack.pop_front() {
            let id = Self::command_object_id(&command);
            self.apply_redo(command.clone(), scene);
            self.undo_stack.push_front(command);
            id
        } else {
            None
        }
    }

    fn command_object_id(command: &EditorCommand) -> Option<Uuid> {
        match command {
            EditorCommand::CreateObject { id, .. } => Some(*id),
            EditorCommand::DeleteObject { id, .. } => Some(*id),
            EditorCommand::ModifyTransform { id, .. } => Some(*id),
            EditorCommand::ModifyMesh { id, .. } => Some(*id),
        }
    }

    fn apply_undo(&self, command: EditorCommand, scene: &mut Scene) {
        match command {
            EditorCommand::CreateObject { id, .. } => { scene.remove_object(id); }
            EditorCommand::DeleteObject { object, .. } => { scene.add_object(object); }
            EditorCommand::ModifyTransform { id, old_transform, .. } => {
                if let Some(obj) = scene.get_object_mut(id) {
                    obj.transform = old_transform;
                }
            }
            EditorCommand::ModifyMesh { id, old_mesh, .. } => {
                if let Some(obj) = scene.get_object_mut(id) {
                    if let crate::scene::ObjectType::Mesh(m) = &mut obj.object_type {
                        m.mesh = old_mesh;
                    }
                }
            }
        }
    }

    fn apply_redo(&self, command: EditorCommand, scene: &mut Scene) {
        match command {
            EditorCommand::CreateObject { object, .. } => { scene.add_object(object); }
            EditorCommand::DeleteObject { id, .. } => { scene.remove_object(id); }
            EditorCommand::ModifyTransform { id, new_transform, .. } => {
                if let Some(obj) = scene.get_object_mut(id) {
                    obj.transform = new_transform;
                }
            }
            EditorCommand::ModifyMesh { id, new_mesh, .. } => {
                if let Some(obj) = scene.get_object_mut(id) {
                    if let crate::scene::ObjectType::Mesh(m) = &mut obj.object_type {
                        m.mesh = new_mesh;
                    }
                }
            }
        }
    }

    pub fn can_undo(&self) -> bool { !self.undo_stack.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo_stack.is_empty() }
}