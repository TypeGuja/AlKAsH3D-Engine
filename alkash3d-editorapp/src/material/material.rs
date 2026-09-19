use super::TextureAsset;

#[derive(Debug, Clone)]
pub struct Material {
    pub name: String,
    pub color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: [f32; 3],
    // ДОБАВЛЕНО (текстуры материалов — по прямому запросу пользователя):
    // изображение albedo-карты, назначенное этому материалу — `None`,
    // если материал красится только сплошным `color` (как раньше, у всех
    // существующих материалов после этой правки поле остаётся `None` по
    // умолчанию, никакого визуального изменения без явного действия
    // пользователя). См. `TextureAsset` и её комментарий про то, как это
    // поле доходит до движка через `.altex`/`.almat`.
    pub albedo_texture: Option<TextureAsset>,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            color: [0.8, 0.8, 0.8, 1.0],
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0, 0.0, 0.0],
            albedo_texture: None,
        }
    }
}