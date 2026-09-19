// src/material/texture_asset.rs
//
// ДОБАВЛЕНО (текстуры материалов — по прямому запросу пользователя:
// "добавь altex чтобы можно было загружать текстуру предмета картинкой ну
// тип у нас есть алмат это материал и тд"): `.almat` уже давно резервирует
// слоты под текстурные карты (`MaterialDefinition::albedo_texture_id` и
// т.д., см. alkash3d-rust/src/almat_format.rs), а `.altex` уже умеет
// встраивать сырые пиксели текстуры прямо в файл и движок уже умеет их
// грузить на GPU и сэмплировать в шейдере (см. `AltexFile::add_texture`/
// `Material::albedo_map` и `asset_loading.rs::load_altex_map_srv` —
// написано ЗАДОЛГО до этой правки, "Задача #15"). Единственное, чего не
// хватало — самого эдитора: `material::Material` не имел ни одного поля
// под изображение, так что назначить текстуру объекту было решительно
// нечем. `TextureAsset` — тот недостающий кусок: декодированное (RGBA8)
// изображение, которое можно (а) показать/использовать в эдиторе, (б)
// встроить в экспортируемый `.altex` как есть, (в) сослаться на него по
// пути из `.almat` (переиспользуемая библиотека материалов хранит ТОЛЬКО
// путь, не пиксели — см. комментарий у `almat_format::MaterialDefinition`).
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};

/// Декодированное изображение в памяти эдитора — всегда RGBA8
/// (`pixels.len() == width * height * 4`), независимо от того, в каком
/// формате был исходный файл на диске (PNG/JPEG/BMP/TGA/...decode делает
/// `image` crate). `Arc`, а не `Vec` — назначение той же самой текстуры
/// нескольким материалам/объектам (обычный случай: один и тот же кирпич
/// на десятках стен) не должно копировать потенциально мегабайтные
/// пиксельные данные при каждом клонировании `Material`.
#[derive(Debug, Clone)]
pub struct TextureAsset {
    /// Путь к исходному файлу на диске, если текстура была загружена
    /// оттуда — используется `.almat` (который ссылается на текстуры по
    /// пути, а не встраивает их, см. комментарий выше) и для повторной
    /// загрузки/отображения в UI ("откуда это"). `None` для текстуры,
    /// пришедшей ТОЛЬКО из встроенных байт `.altex` (например после
    /// импорта чужого `.altex`, для которого исходного файла на диске
    /// эдитора никогда не было) — такую текстуру можно встроить обратно в
    /// `.altex`, но НЕЛЬЗЯ сослаться на неё по пути из `.almat`.
    pub source_path: Option<String>,
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<Vec<u8>>,
}

impl TextureAsset {
    /// Декодирует изображение с диска (PNG/JPEG/BMP/TGA/... — что бы ни
    /// понял `image` crate) и переводит в RGBA8 независимо от исходного
    /// формата (`DynamicImage::to_rgba8()` сама доливает альфу=255 для
    /// форматов без альфа-канала и разворачивает индексированные/серые
    /// палитры) — та же раскладка байт, что и `AltexFile::add_texture`
    /// ожидает на встраивание, и та же, что `image::RgbaImage` даёт без
    /// дополнительных преобразований.
    pub fn load_from_file(path: &str) -> Result<Self> {
        let img = image::open(path)
            .with_context(|| format!("не удалось декодировать изображение '{}'", path))?;
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        if width == 0 || height == 0 {
            return Err(anyhow!("изображение '{}' имеет нулевой размер ({}x{})", path, width, height));
        }
        Ok(Self {
            source_path: Some(path.to_string()),
            width,
            height,
            pixels: Arc::new(rgba.into_raw()),
        })
    }

    /// Строит `TextureAsset` из уже готовых RGBA8-байт (например встроенных
    /// в `.altex`, см. `converters/altex.rs::import_altex`) — без файла на
    /// диске, поэтому `source_path = None` (см. комментарий у поля).
    /// `pixels.len()` обязан быть РОВНО `width*height*4` — вызывающий код
    /// отвечает за это (та же гарантия, что `AltexFile::add_texture`
    /// требует от своих вызывающих).
    pub fn from_rgba(width: u32, height: u32, pixels: Vec<u8>, source_path: Option<String>) -> Self {
        debug_assert_eq!(pixels.len(), width as usize * height as usize * 4);
        Self { source_path, width, height, pixels: Arc::new(pixels) }
    }
}
