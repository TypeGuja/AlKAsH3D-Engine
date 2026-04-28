use std::collections::VecDeque;
use uuid::Uuid;
use crate::math::Transform;
use crate::scene::Scene;

#[derive(Debug, Clone)]
pub enum EditorCommand {
    CreateObject { id: Uuid, object: crate::scene::GameObject },
    DeleteObject { id: Uuid, object: crate::scene::GameObject },
    ModifyTransform { id: Uuid, old_transform: Transform, new_transform: Transform },
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

    pub fn undo(&mut self, scene: &mut Scene) -> bool {
        if let Some(command) = self.undo_stack.pop_front() {
            self.apply_undo(command.clone(), scene);
            self.redo_stack.push_front(command);
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self, scene: &mut Scene) -> bool {
        if let Some(command) = self.redo_stack.pop_front() {
            self.apply_redo(command.clone(), scene);
            self.undo_stack.push_front(command);
            true
        } else {
            false
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
        }
    }

    pub fn can_undo(&self) -> bool { !self.undo_stack.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo_stack.is_empty() }
}