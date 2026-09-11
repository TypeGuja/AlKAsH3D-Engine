// src/plugin/mod.rs
mod abi;
mod physics_api;
mod light_api;
mod scripting_api;
mod manager;

pub use abi::*;
pub use physics_api::*;
pub use light_api::*;
pub use scripting_api::*;
pub use manager::*;

// Вспомогательные структуры для плагинов
use std::ffi::c_void;
use crate::plugin::manager::PluginManager;

pub struct PhysicsPlugin {
    pub api: PhysicsAPI,
    pub instance: *mut c_void,
    manager: PluginManager,
}

impl PhysicsPlugin {
    pub fn load(path: &str, config: PhysicsConfig) -> Result<Self, String> {
        let mut manager = PluginManager::new();
        let config_ptr = &config as *const PhysicsConfig as *const c_void;
        manager.load_plugin(path, std::ptr::null_mut(), config_ptr)?;

        let api = manager.get_physics_api().ok_or("No physics API")?;

        // Получаем instance через PluginManager
        let instance = manager.get_physics_instance().ok_or("No physics instance")?;

        Ok(Self {
            api: *api,
            instance,
            manager,
        })
    }

    pub fn update(&mut self, dt: f32, gravity: f32) {
        (self.api.update)(self.instance, dt, gravity);
    }

    pub fn add_body(&mut self, body: &PhysicsBody) -> i32 {
        (self.api.add_body)(self.instance, body)
    }

    /// ДОБАВЛЕНО (Задача #16 плана — физика и коллизии): обёртка над
    /// `PhysicsAPI::get_body`, уже присутствовавшим в ABI (см.
    /// physics_api.rs) с самой первой версии, но раньше не имевшим
    /// соответствующего метода в безопасной обёртке — ничего в движке его
    /// ни разу не вызывало. Нужен для синхронизации видимой геометрии с
    /// результатом текущего кадра физики (см.
    /// `AlkashEngine::sync_physics_transforms` в engine/mod.rs) — только
    /// через `get_body` движок узнаёт, куда РЕАЛЬНО переместил тело
    /// физический солвер (интегрирование + разрешение столкновений), а не
    /// куда оно было бы без учёта коллизий.
    pub fn get_body(&self, id: i32) -> PhysicsBody {
        (self.api.get_body)(self.instance, id)
    }

    /// ДОБАВЛЕНО (объединённая сцена — физика из .alworld): обёртка над
    /// `PhysicsAPI::remove_body`, тоже присутствовавшим в ABI с самой
    /// первой версии (см. `plugin/physics_api.rs`), но раньше без
    /// безопасной обёртки — до этого момента ни один физический объект,
    /// однажды созданный через `add_body`, не мог быть удалён из плагина
    /// иначе как полной перезагрузкой DLL. Нужен, чтобы
    /// `AlkashEngine::unload_chunk` мог убрать физическое тело объекта
    /// при выгрузке его чанка (см. `ChunkRuntimeState::spawned_physics_bodies`)
    /// — без этого метода тела выгруженных чанков продолжали бы жить в
    /// плагине навсегда, накапливаясь при активном стриминге открытого
    /// мира.
    pub fn remove_body(&mut self, id: i32) {
        (self.api.remove_body)(self.instance, id);
    }

    pub fn get_contacts(&self) -> &[PhysicsContact] {
        unsafe {
            let ptr = (self.api.get_contacts)(self.instance);
            let count = (self.api.get_contacts_count)(self.instance);
            if count > 0 && !ptr.is_null() {
                std::slice::from_raw_parts(ptr, count as usize)
            } else {
                &[]
            }
        }
    }

    /// ДОБАВЛЕНО (диагностика — жалоба пользователя "всё равно ФПС не
    /// радует" ПОСЛЕ фиксов стриминга/hot-reload/culling): обёртка над
    /// `PhysicsAPI::get_stats`, присутствовавшим в ABI плагина с самого
    /// начала (см. `plugin/physics_api.rs`), но раньше без безопасной
    /// обёртки — ничего в движке его ни разу не вызывало, статистика
    /// (broad/narrow phase/solver время, число тел/контактов/пар)
    /// реально считалась внутри `alkash3d-inertial` каждый кадр, но
    /// никогда не покидала плагин. Нужна, чтобы РЕАЛЬНО измерить, где
    /// именно уходит время в кадре, вместо дальнейших догадок по коду.
    pub fn get_stats(&self) -> PhysicsStats {
        (self.api.get_stats)(self.instance)
    }

    /// ДОБАВЛЕНО (разборка машины на детали — джойнты/constraint API):
    /// обёртка над `PhysicsAPI::add_constraint` — создаёт соединение
    /// (шар/петля/сварка/ползун, см. `ConstraintDesc::joint_type`/
    /// `joint_type` в physics_api.rs) между двумя УЖЕ существующими
    /// телами. `None`, если плагин отказал (один из `body_a`/`body_b`
    /// не существует) — тот же принцип "отрицательный id — ошибка,
    /// превращаем в `Option`", что уже применяет
    /// `AlkashEngine::add_physics_body` в `engine/physics_bridge.rs` для
    /// `add_body`.
    pub fn add_constraint(&mut self, desc: &ConstraintDesc) -> Option<i32> {
        let id = (self.api.add_constraint)(self.instance, desc);
        if id >= 0 { Some(id) } else { None }
    }

    /// ДОБАВЛЕНО (код-ревью — статичный коллайдер-плоскость): та же
    /// обёртка "отрицательный id → `None`", что `add_constraint` выше.
    /// См. `PlaneDesc` за объяснением, почему это только для пола.
    pub fn add_plane(&mut self, desc: &PlaneDesc) -> Option<i32> {
        let id = (self.api.add_plane)(self.instance, desc);
        if id >= 0 { Some(id) } else { None }
    }

    /// Удаляет соединение (в т.ч. уже сломанное) по его handle'у — не
    /// затрагивает сами тела.
    pub fn remove_constraint(&mut self, id: i32) {
        (self.api.remove_constraint)(self.instance, id);
    }

    pub fn get_constraint(&self, id: i32) -> ConstraintInfo {
        (self.api.get_constraint)(self.instance, id)
    }

    /// Handle'ы соединений, впервые сломавшихся на ПОСЛЕДНЕМ `update()`
    /// физики — см. подробное объяснение "почему только новые события, а
    /// не весь список сломанных" у `PhysicsAPI::get_broken_constraints`.
    /// Игровой код (например `AlkashEngine`) читает этот список раз за
    /// кадр, чтобы один раз проиграть звук/заспавнить обломок на каждую
    /// поломку.
    pub fn get_broken_constraints(&self) -> &[i32] {
        unsafe {
            // ИСПРАВЛЕНО (код-ревью — гонка указатель/длина): раньше
            // указатель и count читались ДВУМЯ отдельными FFI-вызовами
            // (двумя независимыми lock/unlock мьютекса плагина), что
            // могло дать висячий указатель при пересекающемся `update()`
            // на другом потоке. Теперь один вызов `get_broken_constraints`
            // отдаёт оба значения под одним локом — см. комментарий у
            // этого поля в `physics_api.rs`.
            let mut count: i32 = 0;
            let ptr = (self.api.get_broken_constraints)(self.instance, &mut count);
            if count > 0 && !ptr.is_null() {
                std::slice::from_raw_parts(ptr, count as usize)
            } else {
                &[]
            }
        }
    }

    /// ДОБАВЛЕНО (Фаза 1 реальной физики — см. план "фундамент реальной
    /// физики"): копит силу (Н, мировые координаты) в аккумулятор плагина
    /// ДО следующего `update()` — зови КАЖДЫЙ кадр, пока сила должна
    /// действовать (газ, сопротивление воздуха и т.п.), аккумулятор
    /// обнуляется сразу после интеграции этого кадра. Будит тело, no-op
    /// для static/несуществующего id.
    pub fn apply_force(&mut self, id: i32, force: [f32; 3]) {
        (self.api.apply_force)(self.instance, id, force.as_ptr());
    }

    /// Мгновенно `v += impulse * inv_mass`, в отличие от `apply_force` не
    /// ждёт следующего `update()`.
    pub fn apply_impulse(&mut self, id: i32, impulse: [f32; 3]) {
        (self.api.apply_impulse)(self.instance, id, impulse.as_ptr());
    }

    /// Прямая перезапись линейной/угловой скорости тела (телепорт
    /// скорости).
    pub fn set_velocity(&mut self, id: i32, linear: [f32; 3], angular: [f32; 3]) {
        (self.api.set_velocity)(self.instance, id, linear.as_ptr(), angular.as_ptr());
    }

    /// Прямая перезапись позиции/ориентации тела (телепорт) — скорость НЕ
    /// трогает, зови `set_velocity` отдельно, если нужно ещё и
    /// погасить/задать скорость при телепорте.
    pub fn set_transform(&mut self, id: i32, position: [f32; 3], orientation: [f32; 4]) {
        (self.api.set_transform)(self.instance, id, position.as_ptr(), orientation.as_ptr());
    }

    /// ДОБАВЛЕНО (реальная физика машины — подвеска): копит момент силы
    /// (Н·м) — та же семантика, что у `apply_force`, только угловая.
    pub fn apply_torque(&mut self, id: i32, torque: [f32; 3]) {
        (self.api.apply_torque)(self.instance, id, torque.as_ptr());
    }

    /// Прикладывает силу в точке `world_point` (не через центр масс) —
    /// рождает и линейное ускорение, и момент. Ключевая функция для
    /// честной подвески (сила пружины/демпфера на колесе).
    pub fn apply_force_at_point(&mut self, id: i32, force: [f32; 3], world_point: [f32; 3]) {
        (self.api.apply_force_at_point)(self.instance, id, force.as_ptr(), world_point.as_ptr());
    }

    /// ДОБАВЛЕНО (полноценная физика — запрос луча против сцены): обёртка
    /// над `PhysicsAPI::raycast` — превращает `RaycastHit::hit == 0` в
    /// `None`, тот же принцип, что уже применяют `add_constraint`/
    /// `add_plane` для отрицательного id. `direction` не обязан быть
    /// нормированным — плагин нормирует его сам. `exclude_body` — handle
    /// тела, которое нужно пропустить (см. `RaycastHit`/`PhysicsAPI::raycast`
    /// за подробным обоснованием — как правило, это собственное тело
    /// вызывающего, например кузов машины при raycast'е её подвески).
    pub fn raycast(&self, origin: [f32; 3], direction: [f32; 3], max_dist: f32, exclude_body: Option<i32>) -> Option<RaycastHit> {
        let hit = (self.api.raycast)(self.instance, origin.as_ptr(), direction.as_ptr(), max_dist, exclude_body.unwrap_or(-1));
        if hit.hit != 0 { Some(hit) } else { None }
    }
}

pub struct LightPlugin {
    pub api: LightAPI,
    pub instance: *mut c_void,
    manager: PluginManager,
}

impl LightPlugin {
    pub fn load(path: &str, device_ptr: *mut c_void, config: LightConfig) -> Result<Self, String> {
        let mut manager = PluginManager::new();
        let config_ptr = &config as *const LightConfig as *const c_void;
        manager.load_plugin(path, device_ptr, config_ptr)?;

        let api = manager.get_light_api().ok_or("No light API")?;
        let instance = manager.get_light_instance().ok_or("No light instance")?;

        Ok(Self {
            api: *api,
            instance,
            manager,
        })
    }

    pub fn add_light(&mut self, light: &GPULight) -> u32 {
        (self.api.add_light)(self.instance, light)
    }

    /// ДОБАВЛЕНО (Фаза 7 плана по реализму/фонарям — день/ночь и
    /// мерцание): обёртка над `LightAPI::update_light`, которая уже была
    /// в ABI плагина (см. `plugin/light_api.rs`) с самой первой версии, но
    /// раньше не имела соответствующего метода в безопасной обёртке
    /// `LightPlugin` — ничего в движке её ни разу не вызывало, потому что
    /// раньше свет один раз добавлялся (`add_light`) и больше никогда не
    /// менялся. Мерцание и включение/выключение по времени суток (см.
    /// `AlkashEngine::update_day_night` в engine/mod.rs) требуют менять
    /// intensity/enabled уже добавленного света КАЖДЫЙ кадр — без этого
    /// метода это было бы невозможно без изменения ABI плагина.
    pub fn update_light(&mut self, id: u32, light: &GPULight) {
        (self.api.update_light)(self.instance, id, light);
    }

    pub fn cull(&mut self, camera_pos: [f32; 3], view_proj: &[f32; 16], dt: f32) {
        (self.api.cull)(self.instance, camera_pos.as_ptr(), view_proj.as_ptr(), dt);
    }

    pub fn get_gpu_lights(&self) -> &[GPULight] {
        unsafe {
            let ptr = (self.api.get_gpu_lights)(self.instance);
            let count = (self.api.get_gpu_lights_count)(self.instance);
            if count > 0 && !ptr.is_null() {
                std::slice::from_raw_parts(ptr, count as usize)
            } else {
                &[]
            }
        }
    }

    // ДОБАВЛЕНО (Фаза 3 плана по реализму/фонарям): доступ к
    // пространственной сетке, которую FirstFires уже строит внутри
    // `cull()` (см. LightState::cull в alkash3d-FirstFires/src/lib.rs) —
    // используется, чтобы пиксельный шейдер проверял только фонари своей
    // ячейки, а не перебирал ВЕСЬ видимый список на каждый пиксель (см.
    // render_frame/compile_default_shaders в engine/mod.rs).

    pub fn get_grid_cells(&self) -> &[LightGridCell] {
        unsafe {
            let ptr = (self.api.get_light_grid_cells)(self.instance);
            let count = (self.api.get_grid_cells_count)(self.instance);
            if count > 0 && !ptr.is_null() {
                std::slice::from_raw_parts(ptr, count as usize)
            } else {
                &[]
            }
        }
    }

    pub fn get_grid_entries(&self) -> &[LightGridEntry] {
        unsafe {
            let ptr = (self.api.get_light_grid_entries)(self.instance);
            let count = (self.api.get_grid_entries_count)(self.instance);
            if count > 0 && !ptr.is_null() {
                std::slice::from_raw_parts(ptr, count as usize)
            } else {
                &[]
            }
        }
    }

    pub fn get_grid_params(&self) -> LightGridParams {
        (self.api.get_grid_params)(self.instance)
    }
}

/// ДОБАВЛЕНО (скриптинг, этап 1 — нативные C++/Rust плагины): безопасная
/// обёртка над одной загруженной скриптовой DLL — тот же паттерн, что и
/// `PhysicsPlugin`/`LightPlugin` выше (собственный `PluginManager`,
/// `api`/`instance`, кэшированные из первого `get_scripting_api`/
/// `get_scripting_instance` после загрузки).
///
/// Отличие от Physics/Light: `AlkashEngine` держит НЕСКОЛЬКО
/// `ScriptingPlugin` одновременно (по одному на каждую РАЗНУЮ загруженную
/// DLL — см. `native_script_plugins: HashMap<String, ScriptingPlugin>` в
/// engine/mod.rs), а не один статический экземпляр. Одна и та же
/// `ScriptingPlugin` (одна DLL) может при этом обслуживать НЕСКОЛЬКО
/// прикреплённых сущностей через `create_script`/`script_id`.
pub struct ScriptingPlugin {
    pub api: ScriptingAPI,
    pub instance: *mut c_void,
    manager: PluginManager,
}

impl ScriptingPlugin {
    /// Грузит DLL по `path` и конфигурирует её через `config`. В отличие
    /// от Physics/Light, `device_ptr` скриптам на этом этапе не передаётся
    /// (`std::ptr::null_mut()`) — нативным скриптам первого этапа (движение
    /// сущности + события) прямой доступ к D3D12-устройству не нужен;
    /// расширить сигнатуру, если будущий скрипт всё же захочет рисовать
    /// сам (например debug-визуализация) — тогда это будет ломающее
    /// изменение ABI, требующее поднять PLUGIN_API_VERSION.
    pub fn load(path: &str, config: ScriptConfig) -> Result<Self, String> {
        let mut manager = PluginManager::new();
        let config_ptr = &config as *const ScriptConfig as *const c_void;
        manager.load_plugin(path, std::ptr::null_mut(), config_ptr)?;

        let api = manager.get_scripting_api(path).ok_or("No scripting API")?;
        let instance = manager.get_scripting_instance(path).ok_or("No scripting instance")?;

        Ok(Self {
            api: *api,
            instance,
            manager,
        })
    }

    /// Прикрепляет логику этой DLL к сущности `entity_id` (уже
    /// упакованный, см. `ScriptEvent` в scripting_api.rs) — возвращает
    /// script_id для последующих `update_script`/`dispatch_event`/
    /// `destroy_script`, либо `None`, если плагин отказал (например
    /// `u32::MAX` — превышен `max_scripts` из `ScriptConfig`).
    pub fn create_script(&mut self, entity_id: u64) -> Option<u32> {
        let id = (self.api.create_script)(self.instance, entity_id);
        if id == u32::MAX { None } else { Some(id) }
    }

    /// ДОБАВЛЕНО (скриптинг, вторая волна — Lua как универсальный
    /// DLL-плагин): вариант `create_script` с указанием пути к конкретному
    /// .lua-файлу (см. `ScriptingAPI::create_script_with_source` в
    /// scripting_api.rs) — нужен, потому что одна alkash3d-luascript.dll
    /// обслуживает МНОГО разных .lua-скриптов, в отличие от Native
    /// (alkash3d-examplescript), где вся логика зашита в саму DLL и
    /// обычного `create_script` достаточно. `source_path` конвертируется
    /// в null-terminated C-строку здесь же — плагин не обязан удерживать
    /// указатель дольше самого вызова.
    pub fn create_script_with_source(&mut self, entity_id: u64, source_path: &str) -> Option<u32> {
        let c_path = match std::ffi::CString::new(source_path) {
            Ok(s) => s,
            Err(_) => return None, // путь содержит NUL-байт — некорректные данные
        };
        let id = (self.api.create_script_with_source)(self.instance, entity_id, c_path.as_ptr());
        if id == u32::MAX { None } else { Some(id) }
    }

    pub fn destroy_script(&mut self, script_id: u32) {
        (self.api.destroy_script)(self.instance, script_id);
    }

    /// Заполненный движком `ctx` передаётся по `&mut` — плагин пишет
    /// результат обратно в те же поля (`out_position`/`out_rotation`/
    /// `position_changed`), вызывающая сторона (`AlkashEngine::update`)
    /// сама решает, применять ли их к `Transform`.
    pub fn update_script(&mut self, script_id: u32, ctx: &mut ScriptContext) {
        (self.api.update_script)(self.instance, script_id, ctx as *mut ScriptContext);
    }

    pub fn dispatch_event(&mut self, script_id: u32, event: &ScriptEvent) {
        (self.api.dispatch_event)(self.instance, script_id, event as *const ScriptEvent);
    }

    pub fn get_active_scripts_count(&self) -> u32 {
        (self.api.get_active_scripts_count)(self.instance)
    }
}