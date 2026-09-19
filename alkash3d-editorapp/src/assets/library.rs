use std::collections::HashMap;
use std::path::Path;
use crate::mesh::Mesh;
use crate::material::Material;

pub struct AssetLibrary {
    pub meshes: HashMap<String, Mesh>,
    pub materials: HashMap<String, Material>,
}

impl AssetLibrary {
    pub fn new() -> Self {
        let mut meshes = HashMap::new();
        meshes.insert("cube".to_string(), Mesh::create_cube());
        meshes.insert("sphere".to_string(), Mesh::create_sphere());
        meshes.insert("plane".to_string(), Mesh::create_plane());
        meshes.insert("cylinder".to_string(), Mesh::create_cylinder());
        meshes.insert("cone".to_string(), Mesh::create_cone());
        meshes.insert("torus".to_string(), Mesh::create_torus());

        let mut materials = HashMap::new();
        materials.insert("default".to_string(), Material::default());
        materials.insert("red".to_string(), Material { color: [1.0, 0.2, 0.2, 1.0], ..Default::default() });
        materials.insert("green".to_string(), Material { color: [0.2, 1.0, 0.2, 1.0], ..Default::default() });
        materials.insert("blue".to_string(), Material { color: [0.2, 0.2, 1.0, 1.0], ..Default::default() });
        materials.insert("metal".to_string(), Material { metallic: 1.0, roughness: 0.3, ..Default::default() });
        
        Self { meshes, materials }
    }

    pub fn import_model(&mut self, path: &str) -> Result<Vec<String>, String> {
        let file_name = Path::new(path)
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed")
            .to_string();

        println!("[ASSET] Importing model: {} (name: {})", path, file_name);

        let mesh = self.parse_obj(path)?;
        self.meshes.insert(file_name.clone(), mesh);
        Ok(vec![file_name])
    }

    fn parse_obj(&self, path: &str) -> Result<Mesh, String> {
        println!("[ASSET] Parsing OBJ: {}", path);

        // Проверяем существование файла
        if !Path::new(path).exists() {
            return Err(format!("File not found: {}", path));
        }

        let (models, materials) = tobj::load_obj(
            path,
            &tobj::LoadOptions {
                single_index: true,
                triangulate: true,
                ignore_points: true,
                ignore_lines: true,
            },
        ).map_err(|e| {
            println!("[ASSET] tobj error: {}", e);
            format!("Failed to load OBJ: {}", e)
        })?;

        let _ = materials; // Не используем пока

        println!("[ASSET] Loaded {} models", models.len());

        if models.is_empty() {
            return Err("No models found in OBJ file".to_string());
        }

        // Объединяем все модели в один меш (или берем первую)
        let mut all_vertices = Vec::new();
        let mut all_indices = Vec::new();
        let mut vertex_offset = 0u32;
        // ДОБАВЛЕНО (текстуры материалов — по прямому запросу пользователя):
        // tobj парсит `vt`-координаты из .obj не хуже позиций/индексов, но
        // раньше этот код их даже не читал — итоговый `Mesh` всегда получал
        // только автоматическую планарную UV-проекцию из `Mesh::new()`
        // (см. `mesh/uv.rs::recalculate_uv`), даже когда у модели была
        // честная развёртка автора. Собираем `all_uvs` параллельно
        // `all_vertices` и, если ОНА ЕСТЬ У ВСЕХ моделей файла целиком,
        // подменяем ею автоматическую проекцию через `Mesh::set_uv()` ниже
        // — частичная/отсутствующая развёртка у части моделей молча
        // оставляет автоматическую проекцию для ВСЕГО меша (лучше
        // консистентный fallback, чем наполовину честная, наполовину
        // произвольная UV на одном объекте).
        let mut all_uvs = Vec::new();
        let mut all_models_have_uv = true;

        for (idx, model) in models.iter().enumerate() {
            let mesh = &model.mesh;
            let vertex_count = mesh.positions.len() / 3;

            println!("[ASSET] Model {}: {} vertices, {} indices",
                     idx, vertex_count, mesh.indices.len());

            if vertex_count == 0 {
                continue;
            }

            // Добавляем вершины
            for i in 0..vertex_count {
                all_vertices.push(crate::math::Vec3::new(
                    mesh.positions[i * 3],
                    mesh.positions[i * 3 + 1],
                    mesh.positions[i * 3 + 2],
                ));
            }

            let has_texcoords = mesh.texcoords.len() >= vertex_count * 2;
            all_models_have_uv &= has_texcoords;
            if has_texcoords {
                // OBJ (как и большинство форматов, унаследовавших OpenGL-
                // конвенцию) хранит V снизу вверх, а движок/материалы этого
                // редактора ожидают V сверху вниз (та же причина уже была
                // учтена в неиспользуемом converters/obj.rs) — переворачиваем.
                for i in 0..vertex_count {
                    all_uvs.push([mesh.texcoords[i * 2], 1.0 - mesh.texcoords[i * 2 + 1]]);
                }
            }

            // Добавляем индексы со смещением
            for &idx in &mesh.indices {
                all_indices.push(idx as u32 + vertex_offset);
            }

            vertex_offset += vertex_count as u32;
        }

        if all_vertices.is_empty() {
            return Err("No vertices in model".to_string());
        }

        println!("[ASSET] Total: {} vertices, {} indices",
                 all_vertices.len(), all_indices.len());

        let mut result = Mesh::new(all_vertices, all_indices);
        if all_models_have_uv {
            result.set_uv(all_uvs);
        }
        Ok(result)
    }

    pub fn get_mesh(&self, name: &str) -> Option<&Mesh> {
        self.meshes.get(name)
    }

    pub fn list_meshes(&self) -> Vec<String> {
        self.meshes.keys().cloned().collect()
    }

    pub fn list_materials(&self) -> Vec<String> {
        self.materials.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ДОБАВЛЕНО (по прямому запросу пользователя: "попробуй теперь сделать
    /// фонарный столб obj файлом") — `assets/models/street_lamp.obj`,
    /// процедурно сгенерированный тестовый пропс (основание+столб+рычаг+
    /// корпус плафона+рассеиватель, честные `vt`/`vn` в файле), нужен для
    /// того, чтобы реально прогнать через `parse_obj` НЕ примитив редактора,
    /// а импортированную модель — именно тот путь, для которого делалась
    /// правка "не выбрасывать texcoords из tobj" (см. `parse_obj` выше).
    /// Регрессионный тест держит две вещи разом: (1) файл вообще валиден
    /// для tobj и парсится без ошибок, (2) UV реально долетают до
    /// `Mesh::uv`, а не остаются автоматической fallback-проекцией
    /// (`recalculate_uv()`), которую `set_uv()` должен была подменить.
    #[test]
    fn street_lamp_obj_imports_with_real_uv() {
        let mut library = AssetLibrary::new();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/models/street_lamp.obj");

        let names = library.import_model(path).expect("street_lamp.obj must import");
        assert_eq!(names, vec!["street_lamp".to_string()]);

        let mesh = library.get_mesh("street_lamp").expect("imported mesh must be registered");
        assert!(!mesh.vertices.is_empty());
        assert_eq!(mesh.uv.len(), mesh.vertices.len());
        assert_eq!(mesh.normals.len(), mesh.vertices.len());
        assert!(!mesh.indices.is_empty());
        assert_eq!(mesh.indices.len() % 3, 0);

        // Модель ~3.5 м в высоту (столб+рычаг+плафон), стоит на y=0 — грубая
        // проверка масштаба/происхождения, а не точных чисел геометрии.
        let (min, max) = mesh.bounds;
        assert!(min.y.abs() < 0.05, "base should sit on y=0, got {}", min.y);
        assert!(max.y > 3.0 && max.y < 4.0, "lamp head should be ~3-4m up, got {}", max.y);

        // Настоящая UV-развёртка из файла, а не автоматическая fallback-
        // проекция `recalculate_uv()` (планарная проекция по доминирующей
        // оси нормали КАЖДОЙ вершины, нормализованная в [0,1] по bounds
        // ВСЕГО меша — см. mesh/uv.rs). У КАЖДОГО кольца каждого цилиндра
        // (основание/столб/рычаг/рассеиватель) v-координата в файле — РОВНО
        // 0.0 (нижнее кольцо) или РОВНО 1.0 (верхнее) у ВСЕХ вершин кольца
        // разом (десятки вершин на кольцо) — у fallback-проекции ровно 0/1
        // попадает почти исключительно в единственную самую крайнюю по Y
        // вершину всего меша, а не во множество вершин на разных кольцах
        // на разной высоте. Это надёжно отличает "долетевший до Mesh::uv
        // файл" от "set_uv() тихо не сработал, автопроекция осталась".
        let exact_edge_v = mesh.uv.iter().filter(|uv| uv[1] == 0.0 || uv[1] == 1.0).count();
        assert!(
            exact_edge_v > 20,
            "expected many vertices with exact v=0/1 from real per-ring cylinder UV, got {} (looks like the planar fallback projection, not the imported UV)",
            exact_edge_v
        );
    }
}