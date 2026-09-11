//! Мост между движком и внешними плагинами физики (Inertial)/света
//! (FirstFires), плюс встроенный (не-плагинный) звук — загрузка плагинов,
//! добавление физических тел, спавн физической сферы/машины "всё-в-одном",
//! синхронизация физики со сценой (`sync_physics_transforms`).
//!
//! ВЫНЕСЕНО из `engine/mod.rs` (Фаза 1 архитектурного рефакторинга — разбивка
//! монолита `impl AlkashEngine` на подсистемы). Перенос дословный, тела
//! методов не менялись.

use windows::core::*;
use windows::Win32::Foundation::*;
use crate::plugin::{PhysicsPlugin, LightPlugin, PhysicsConfig, LightConfig, GPULight, PhysicsBody, PhysicsContact, PhysicsStats, ConstraintDesc, ConstraintInfo, PlaneDesc, RaycastHit, joint_type};
use crate::math::Vec3;
use crate::audio::AudioEngine;
use super::{AlkashEngine, CarHandle, quaternion_to_euler_zyx};

impl AlkashEngine {
    /// ИСПРАВЛЕНО (Задача #16 плана — физика и коллизии): раньше путь к
    /// плагину был захардкожен как `"plugins/inertial.dll"` внутри этого
    /// метода — ни такой папки, ни файла с таким именем реально не
    /// существовало (физика ни разу не вызывалась ни одним bin/*.rs,
    /// ровно та же ситуация, что была с `init_lights`/FirstFires до
    /// соответствующего фикса, см. её комментарий чуть ниже). Теперь путь
    /// передаётся параметром — тем же способом и с тем же fallback на
    /// `deps/` (см. `deps_fallback_path`), что и `init_lights`, вместо
    /// того чтобы изобретать второй, чуть отличающийся механизм для
    /// второго плагина.
    pub fn init_physics(&mut self, dll_path: &str, config: PhysicsConfig) -> Result<()> {
        let fallback_path = Self::deps_fallback_path(dll_path);
        let primary_exists = std::path::Path::new(dll_path).exists();

        let (used_path, load_result) = if primary_exists {
            (dll_path.to_string(), PhysicsPlugin::load(dll_path, config))
        } else if let Some(fallback) = &fallback_path {
            eprintln!(
                "[ENGINE] '{}' не найден, пробую запасной путь '{}' (Cargo не скопировал .dll из deps/)",
                dll_path, fallback
            );
            (fallback.clone(), PhysicsPlugin::load(fallback, config))
        } else {
            (dll_path.to_string(), PhysicsPlugin::load(dll_path, config))
        };

        match load_result {
            Ok(plugin) => {
                self.physics = Some(plugin);
                println!("[ENGINE] ✓ Physics plugin loaded from '{}'", used_path);
                Ok(())
            }
            Err(e) => {
                eprintln!("[ENGINE] Failed to load physics plugin '{}': {}", used_path, e);
                // ИСПРАВЛЕНО (диагностика "LoadLibraryExW failed"): раньше
                // здесь всегда терялось реальное сообщение об ошибке —
                // `Error::from_hresult(HRESULT(1))` печатает через `{:?}`
                // ОБЩИЙ текст произвольного кода 1 ("Неверная функция"),
                // никак не связанный с настоящей причиной (например
                // ERROR_MOD_NOT_FOUND из-за отсутствующей рантайм-DLL).
                // `eprintln!` выше и так печатает `e`, но только в лог —
                // теперь то же самое сообщение попадает и в саму ошибку,
                // которую видит вызывающий код (см. `{:?}` в main_car.rs).
                Err(Error::new(HRESULT(1), format!("physics plugin load failed: {e}")))
            }
        }
    }

    /// ДОБАВЛЕНО (диагностика — жалоба пользователя "всё равно ФПС не
    /// радует" ПОСЛЕ фиксов стриминга/hot-reload/culling): публичный
    /// доступ к статистике физики этого кадра (см. `PhysicsPlugin::get_stats`)
    /// — `None`, если физика не инициализирована. Позволяет вызывающему
    /// коду (см. `run_loop` в bin/main.rs) реально ИЗМЕРИТЬ время
    /// broad/narrow phase и солвера, число тел/активных тел/контактов/пар,
    /// вместо того чтобы гадать по одной лишь позиции тестовых сфер.
    pub fn physics_stats(&self) -> Option<PhysicsStats> {
        self.physics.as_ref().map(|p| p.get_stats())
    }

    /// ДОБАВЛЕНО (диагностика — жалоба "ФПС скачет, пока камера стоит на
    /// месте" ПОСЛЕ фиксов стриминга/hot-reload/culling/физики): отдаёт
    /// накопленную за текущее окно разбивку `update()` по под-фазам (см.
    /// `UpdateBreakdownMs`/макрос `timed!` внутри `update()`) и СРАЗУ
    /// сбрасывает её в нули — тот же паттерн, что уже `max_update_ms`/
    /// `max_render_ms` в bin/main.rs (там ручной сброс раз в секунду
    /// снаружи; здесь сброс происходит прямо тут, чтобы вызывающий код в
    /// bin/main.rs не мог забыть его сделать и не удвоил логику двух
    /// разных мест сброса).
    pub fn take_update_breakdown(&mut self) -> super::UpdateBreakdownMs {
        std::mem::take(&mut self.update_breakdown_ms)
    }

    /// ИСПРАВЛЕНО (обновление bin/*.rs под текущий движок): раньше путь к
    /// плагину был захардкожен как `"plugins/firstfires.dll"` — ни такой
    /// папки, ни файла с таким именем реально не существует. FirstFires —
    /// отдельный крейт `alkash3d-firstfires` (см. его Cargo.toml), Cargo
    /// заменяет дефисы на подчёркивания в имени артефакта, поэтому
    /// настоящий файл называется `alkash3d_firstfires.dll` и лежит в
    /// `alkash3d-FirstFires/target/<profile>/` — СОСЕДНЕЙ папке относительно
    /// `alkash3d-rust` (обе — подпапки одного репозитория), а не в
    /// подпапке `plugins/` внутри `alkash3d-rust`. Раньше это не
    /// проявлялось как ошибка ТОЛЬКО потому, что `init_lights()` нигде не
    /// вызывался — ни один bin/*.rs не подключал фонари. Теперь путь
    /// передаётся параметром (а не хардкодится здесь) — вызывающий код
    /// (main.rs) сам знает свою рабочую директорию относительно репозитория.
    ///
    /// ДОБАВЛЕНО: на практике на реальной машине Cargo иногда не копирует
    /// готовую `.dll` из `target/<profile>/deps/` в верхний уровень
    /// `target/<profile>/` (там остаются только `.dll.exp`/`.dll.lib`/
    /// `.pdb`, а сама `.dll` — только в `deps/`) — похоже на гонку с файлом,
    /// занятым предыдущим запущенным процессом движка на Windows во время
    /// пересборки. Поэтому если `dll_path` не найден, пробуем тот же файл
    /// внутри соседней папки `deps/` (тот же каталог + "deps/" + имя файла)
    /// как запасной вариант, прежде чем сдаваться.
    pub fn init_lights(&mut self, dll_path: &str, device_ptr: *mut std::ffi::c_void, config: LightConfig) -> Result<()> {
        let fallback_path = Self::deps_fallback_path(dll_path);
        let primary_exists = std::path::Path::new(dll_path).exists();

        let (used_path, load_result) = if primary_exists {
            (dll_path.to_string(), LightPlugin::load(dll_path, device_ptr, config))
        } else if let Some(fallback) = &fallback_path {
            eprintln!(
                "[ENGINE] '{}' не найден, пробую запасной путь '{}' (Cargo не скопировал .dll из deps/)",
                dll_path, fallback
            );
            (fallback.clone(), LightPlugin::load(fallback, device_ptr, config))
        } else {
            (dll_path.to_string(), LightPlugin::load(dll_path, device_ptr, config))
        };

        match load_result {
            Ok(plugin) => {
                self.lights = Some(plugin);
                println!("[ENGINE] ✓ Light plugin loaded from '{}'", used_path);
                Ok(())
            }
            Err(e) => {
                eprintln!("[ENGINE] Failed to load light plugin '{}': {}", used_path, e);
                // См. комментарий у аналогичного места в init_physics выше.
                Err(Error::new(HRESULT(1), format!("light plugin load failed: {e}")))
            }
        }
    }

    /// ДОБАВЛЕНО (звуковая подсистема — Фаза "Sound" плана): создаёт
    /// `AudioEngine` (XAudio2 + mastering voice) — в отличие от
    /// `init_physics`/`init_lights`, не грузит внешний DLL (нет пути/
    /// device_ptr параметров) — сам XAudio2 является системным API, не
    /// плагином движка. Безопасно вызывать даже на системах без звукового
    /// устройства вообще (XAudio2 создаёт "null" render endpoint в этом
    /// случае, а не возвращает ошибку) — так что `init_audio` не должен
    /// проваливаться на "железе 10-летней давности" из ТЗ движка, даже
    /// если у конкретной машины отключена/не установлена звуковая карта.
    pub fn init_audio(&mut self) -> Result<()> {
        match AudioEngine::new() {
            Ok(engine) => {
                self.audio = Some(engine);
                Ok(())
            }
            Err(e) => {
                eprintln!("[ENGINE] Failed to initialize audio engine: {}", e);
                Err(Error::from_hresult(HRESULT(1)))
            }
        }
    }

    /// Загружает `.alsnd` банк звуков + связанные `.wav` файлы (см.
    /// `AudioEngine::load_bank`) — не делает ничего (тихо возвращает 0),
    /// если `init_audio()` ещё не вызывался, тот же принцип "деградируем
    /// молча, не паникуем", что и у остальных опциональных плагинов
    /// движка при обращении к ним до инициализации.
    pub fn load_sound_bank(&mut self, alsnd_path: &str, base_dir: &str) -> Result<usize> {
        match &mut self.audio {
            Some(audio) => audio.load_bank(alsnd_path, base_dir).map_err(|e| {
                eprintln!("[ENGINE] Failed to load sound bank '{}': {}", alsnd_path, e);
                Error::from_hresult(HRESULT(1))
            }),
            None => {
                eprintln!("[ENGINE] load_sound_bank called before init_audio() — банк не загружен");
                Ok(0)
            }
        }
    }

    /// Проигрывает звук по имени из загруженного банка (см.
    /// `AudioEngine::play_sound_by_name`) — `None`, если аудио-движок не
    /// инициализирован, звук не найден, или достигнут лимит
    /// `max_instances` (диагностика печатается в stderr самим
    /// `AudioEngine`, чтобы вызывающий код мог не проверять возврат на
    /// каждый выстрел/шаг, если ему не критично знать об отказе).
    pub fn play_sound(&mut self, name: &str, position: Vec3) -> Option<crate::audio::SoundHandle> {
        let audio = self.audio.as_mut()?;
        match audio.play_sound_by_name(name, position) {
            Ok(handle) => Some(handle),
            Err(e) => {
                eprintln!("[ENGINE] play_sound('{}') failed: {}", name, e);
                None
            }
        }
    }

    /// Строит запасной путь вида `.../target/<profile>/deps/<файл>.dll` из
    /// исходного `.../target/<profile>/<файл>.dll`, вставляя "deps" перед
    /// именем файла. Возвращает `None`, если у пути нет родительской папки
    /// (не должно происходить для реальных путей вида
    /// "../alkash3d-FirstFires/target/release/alkash3d_firstfires.dll").
    ///
    /// ИЗМЕНЕНО (рефакторинг — вынос скриптинга в engine/scripting.rs):
    /// было `fn` (приватная в пределах mod.rs) — `pub(super)` вместо
    /// `fn`/`pub`, потому что приватность в Rust действует по МОДУЛЯМ, а
    /// не по типу: `impl AlkashEngine` в scripting.rs (отдельный
    /// подмодуль `engine::scripting`) вызывает `Self::deps_fallback_path`
    /// и без `pub(super)` не увидел бы приватный метод соседнего модуля,
    /// даже у того же самого типа. `pub(super)` — “видно предку модуля
    /// engine и его подмодулям”, не публичный API крейта наружу.
    pub(super) fn deps_fallback_path(dll_path: &str) -> Option<String> {
        let path = std::path::Path::new(dll_path);
        let file_name = path.file_name()?;
        let parent = path.parent()?;
        Some(parent.join("deps").join(file_name).to_string_lossy().into_owned())
    }

    /// ИСПРАВЛЕНО (краш видеодрайвера при 2025 телах, воспроизведено
    /// пользователем): раньше был `.map(|p| p.add_body(&body))` —
    /// `PhysicsPlugin::add_body` возвращает голый `i32` (не `Option`),
    /// поэтому `.map()` заворачивал ЛЮБОЙ результат, включая `-1` (код
    /// ошибки — отказ, например при переполнении `max_bodies`, см. фикс
    /// в `PhysicsState::add_body` в alkash3d-inertial/src/lib.rs), в
    /// `Some(-1)`. Вызывающий код (`add_sphere_body`/`spawn_physics_car`
    /// и весь код, полагающийся на `.is_some()`) читал `Some(-1)` как
    /// "тело успешно создано с id=-1" — то есть переполнение физики
    /// молчаливо ИГНОРИРОВАЛОСЬ на уровне движка, а не приводило к
    /// понятной ошибке/предупреждению. `.and_then()` теперь честно
    /// превращает отрицательный id (единственный код ошибки в этом ABI,
    /// см. `api_add_body` в inertial) в `None`.
    pub fn add_physics_body(&mut self, body: PhysicsBody) -> Option<i32> {
        self.physics.as_mut().and_then(|p| {
            let id = p.add_body(&body);
            if id >= 0 { Some(id) } else { None }
        })
    }

    pub fn add_sphere_body(&mut self, x: f32, y: f32, z: f32, mass: f32) -> Option<i32> {
        let body = PhysicsBody {
            position: [x, y, z],
            velocity: [0.0; 3],
            acceleration: [0.0; 3],
            angular_velocity: [0.0; 3],
            angular_acceleration: [0.0; 3],
            mass,
            inv_mass: if mass > 0.0 { 1.0 / mass } else { 0.0 },
            restitution: 0.5,
            friction: 0.5,
            linear_damping: 0.01,
            angular_damping: 0.01,
            is_static: if mass <= 0.0 { 1 } else { 0 },
            is_asleep: 0,
            orientation: [0.0, 0.0, 0.0, 1.0],
            // ИСПРАВЛЕНО (E0063 — `PhysicsBody::radius` добавили полем
            // структуры, но забыли обновить конструкторов): `add_sphere_body`
            // — буквальный конструктор сферы старого (до появления поля)
            // поведения, когда каждое тело на Fortran-стороне ВСЕГДА было
            // сферой фиксированного `IMPLICIT_RADIUS = 0.5` (см. комментарий
            // у поля `radius` в `physics_api.rs`). У функции нет параметра
            // радиуса — сохраняем именно то старое значение 0.5, а не
            // произвольную константу: вызывающий код (например
            // `FLOOR_SPHERE_SPACING`/`SPACING` в main.rs/main_car.rs) уже
            // жёстко рассчитан на шаг между сферами-полом ИСХОДЯ из радиуса
            // 0.5 (`< 2*IMPLICIT_RADIUS`) — другое значение здесь молча
            // рассинхронизировало бы плотность стыковки сфер-пола с этими
            // константами.
            radius: 0.5,
            // ИСПРАВЛЕНО (E0063 — те же два новых поля, добавленные для
            // box-коллайдера кузова машины): `add_sphere_body` — буквально
            // сфера, `half_extents` для неё не имеет смысла.
            shape_type: crate::plugin::shape_type::SPHERE,
            half_extents: [0.0; 3],
        };
        self.add_physics_body(body)
    }

    /// ДОБАВЛЕНО (реальная физика машины — box-коллайдер кузова, см.
    /// `PhysicsBody::shape_type`): тот же паттерн, что `add_sphere_body`
    /// выше, но для коробки — `half_extents` задаёт половинные размеры по
    /// локальным осям тела (те же оси, что и у визуального
    /// `Transform.scale`, если меш — единичный куб, см. `add_cube_colored`).
    /// Момент инерции считается на Fortran-стороне (`compute_local_inertia`
    /// в `alkash3d-inertial/src/lib.rs`) из `half_extents`, а не из
    /// `radius` — `radius` здесь не используется совсем (0.0, безопасное
    /// значение по умолчанию).
    pub fn add_box_body(&mut self, x: f32, y: f32, z: f32, mass: f32, half_extents: [f32; 3]) -> Option<i32> {
        let body = PhysicsBody {
            position: [x, y, z],
            velocity: [0.0; 3],
            acceleration: [0.0; 3],
            angular_velocity: [0.0; 3],
            angular_acceleration: [0.0; 3],
            mass,
            inv_mass: if mass > 0.0 { 1.0 / mass } else { 0.0 },
            restitution: 0.1,
            friction: 0.7,
            linear_damping: 0.05,
            angular_damping: 0.35,
            is_static: if mass <= 0.0 { 1 } else { 0 },
            is_asleep: 0,
            orientation: [0.0, 0.0, 0.0, 1.0],
            radius: 0.0,
            shape_type: crate::plugin::shape_type::BOX,
            half_extents,
        };
        self.add_physics_body(body)
    }

    /// ДОБАВЛЕНО (Задача #16 плана — физика и коллизии): удобный
    /// "всё-в-одном" хелпер для демо/игрового кода — создаёт физическое
    /// тело-сферу (через `add_sphere_body`, та же формула inv_mass/
    /// is_static), СРАЗУ создаёт для него видимый меш-инстанс
    /// (`spawn_mesh_entity`) и регистрирует связь в `physics_links`, чтобы
    /// `sync_physics_transforms()` начал обновлять его `Transform` со
    /// следующего кадра. Без этого хелпера пришлось бы вручную
    /// синхронизировать три вызова (`add_sphere_body` +
    /// `spawn_mesh_entity` + `physics_links.push`) в каждом месте
    /// демо-кода, что легко забыть или рассинхронизировать.
    ///
    /// Возвращает `None`, если физический плагин не загружен
    /// (`init_physics` не вызывался или провалился) — в этом случае НЕ
    /// создаёт и визуальную сущность тоже, чтобы не оставлять "мёртвую"
    /// геометрию без физики за спиной у вызывающего кода.
    pub fn spawn_physics_sphere(&mut self, mesh_index: usize, x: f32, y: f32, z: f32, mass: f32) -> Option<(i32, crate::scene::EntityId)> {
        let body_id = self.add_sphere_body(x, y, z, mass)?;
        let entity = self.spawn_mesh_entity(mesh_index);
        if let Some(t) = self.scene.transform_mut(entity) {
            t.position = [x, y, z];
        }
        self.physics_links.push((body_id, entity));
        Some((body_id, entity))
    }

    /// ДОБАВЛЕНО (полноценная физика — capsule-коллайдер, для контроллера
    /// персонажа): тот же паттерн, что `add_sphere_body`/`add_box_body`
    /// выше, но для капсулы — `radius` и `half_height` (полувысота
    /// ЦИЛИНДРИЧЕСКОЙ части вдоль ЛОКАЛЬНОЙ оси Y тела, см.
    /// `shape_type::CAPSULE`) задают форму напрямую, без промежуточной
    /// структуры `PhysicsBody` на стороне вызывающего кода.
    pub fn add_capsule_body(&mut self, x: f32, y: f32, z: f32, mass: f32, radius: f32, half_height: f32) -> Option<i32> {
        let body = PhysicsBody {
            position: [x, y, z],
            velocity: [0.0; 3],
            acceleration: [0.0; 3],
            angular_velocity: [0.0; 3],
            angular_acceleration: [0.0; 3],
            mass,
            inv_mass: if mass > 0.0 { 1.0 / mass } else { 0.0 },
            restitution: 0.1,
            friction: 0.6,
            linear_damping: 0.05,
            angular_damping: 0.35,
            is_static: if mass <= 0.0 { 1 } else { 0 },
            is_asleep: 0,
            orientation: [0.0, 0.0, 0.0, 1.0],
            radius,
            shape_type: crate::plugin::shape_type::CAPSULE,
            half_extents: [half_height, 0.0, 0.0],
        };
        self.add_physics_body(body)
    }

    /// ДОБАВЛЕНО (задача #39 плана — модель машины: кузов + 4 колеса):
    /// тот же паттерн "всё-в-одном", что и `spawn_physics_sphere` выше, но
    /// сразу с иерархией "кузов + 4 колеса", по образцу My Summer Car
    /// (СВОЯ, независимая реализация — не копия чужого кода/ассетов, см.
    /// обсуждение подхода с пользователем).
    ///
    /// Физическое тело ОДНО — это кузов (`add_physics_body`, РЕАЛЬНАЯ
    /// коробка нужного размера — см. `PhysicsBody::shape_type`, узкая фаза
    /// понимает box-vs-sphere/box-vs-plane в дополнение к старому
    /// sphere-sphere). Колёса пока не самостоятельные rigid body — сила
    /// подвески на каждое колесо прикладывается К КУЗОВУ через
    /// `apply_physics_force_at_point` в мировой точке колеса (см.
    /// `wheel_local_positions`/`wheel_radius` в `CarHandle`), честный
    /// raycast+пружина-демпфер считается игровым кодом (main_car.rs), а не
    /// этой функцией — она только создаёт тело и визуальную иерархию.
    /// Колёса — ЧИСТО визуальные
    /// ECS-сущности без своей физики, подвешенные как ДЕТИ кузова через
    /// `Scene::set_parent` — значит их мировая позиция/поворот всегда
    /// автоматически следуют за кузовом через `for_each_world_transform`
    /// (см. scene.rs), включая вращение кузова (кватернион из
    /// `sync_physics_transforms`/`quaternion_to_euler_zyx`), без ручной
    /// синхронизации на каждый кадр.
    ///
    /// `chassis_mesh`/`wheel_mesh` — индексы уже созданных мешей (обычно
    /// `add_cube_colored`, см. `AlkashEngine::add_car_demo_meshes` ниже
    /// для готового набора). `half_extents` — половина размеров кузова
    /// по осям (X=ширина, Y=высота, Z=длина) в метрах, применяется как
    /// `Transform.scale` кузова (сам меш — единичный куб, см.
    /// `Mesh::cube_colored(1.0, ...)`), поэтому scale = размер В МЕТРАХ,
    /// а не половина — умножаем на 2.0 ниже). `wheel_radius`/
    /// `wheel_width` — размеры колеса (тоже как приплюснутый куб —
    /// цилиндра в движке нет, см. `Mesh` в этом файле).
    ///
    /// Возвращает `None` на тех же условиях, что и `spawn_physics_sphere`
    /// (физика не инициализирована) — в этом случае не создаёт ничего
    /// визуального тоже.
    pub fn spawn_physics_car(
        &mut self,
        chassis_mesh: usize,
        wheel_mesh: usize,
        x: f32,
        y: f32,
        z: f32,
        mass: f32,
        half_extents: [f32; 3],
        wheel_radius: f32,
        wheel_width: f32,
    ) -> Option<CarHandle> {
        let body = PhysicsBody {
            position: [x, y, z],
            velocity: [0.0; 3],
            acceleration: [0.0; 3],
            angular_velocity: [0.0; 3],
            angular_acceleration: [0.0; 3],
            mass,
            inv_mass: if mass > 0.0 { 1.0 / mass } else { 0.0 },
            restitution: 0.1,
            friction: 0.6,
            linear_damping: 0.02,
            angular_damping: 0.15,
            is_static: if mass <= 0.0 { 1 } else { 0 },
            is_asleep: 0,
            orientation: [0.0, 0.0, 0.0, 1.0],
            // ИСПРАВЛЕНО (реальная физика машины — box-коллайдер, см.
            // `PhysicsBody::shape_type`): раньше здесь была сфера, ОПИСАННАЯ
            // вокруг `half_extents`-коробки (диагональ половины коробки) —
            // приближение, у которого по углам кузов ложно "касался" раньше
            // геометрической границы. Теперь узкая фаза честно знает про
            // box-vs-sphere/box-vs-plane (см. `alkash3d-inertial/src/lib.rs`),
            // так что кузов — РЕАЛЬНАЯ коробка нужного размера, `radius` для
            // неё не используется вообще (не имеет смысла для box-тела).
            radius: 0.0,
            shape_type: crate::plugin::shape_type::BOX,
            half_extents,
        };
        let body_id = self.add_physics_body(body)?;

        let chassis_entity = self.spawn_mesh_entity(chassis_mesh);
        if let Some(t) = self.scene.transform_mut(chassis_entity) {
            t.position = [x, y, z];
            t.scale = [
                half_extents[0] * 2.0,
                half_extents[1] * 2.0,
                half_extents[2] * 2.0,
            ];
        }
        self.physics_links.push((body_id, chassis_entity));

        let wheel_x = half_extents[0] + wheel_width * 0.5;
        let wheel_y = -half_extents[1] + wheel_radius * 0.3;
        let wheel_z = half_extents[2] * 0.65;

        let wheel_local_positions: [[f32; 3]; 4] = [
            [-wheel_x, wheel_y, wheel_z],
            [wheel_x, wheel_y, wheel_z],
            [-wheel_x, wheel_y, -wheel_z],
            [wheel_x, wheel_y, -wheel_z],
        ];

        let mut wheel_entities = [crate::scene::EntityId::INVALID; 4];
        for (i, local_pos) in wheel_local_positions.iter().enumerate() {
            let wheel = self.spawn_mesh_entity(wheel_mesh);
            self.scene.set_parent(wheel, Some(chassis_entity));
            if let Some(t) = self.scene.transform_mut(wheel) {
                t.position = *local_pos;
                t.scale = [wheel_width, wheel_radius * 2.0, wheel_radius * 2.0];
            }
            wheel_entities[i] = wheel;
        }

        Some(CarHandle {
            body_id,
            chassis_entity,
            wheel_entities,
            wheel_local_positions,
            wheel_radius,
        })
    }

    /// ДОБАВЛЕНО (задача #39 плана): создаёт стандартный набор мешей для
    /// `spawn_physics_car` — один раз для всех машин сцены (в отличие от
    /// `add_cube_colored`, вызванного отдельно на каждую машину, что
    /// впустую тратило бы GPU-память на идентичную геометрию). Цвета —
    /// просто разумные значения по умолчанию: тёмно-красный кузов,
    /// почти чёрные колёса — вызывающий код может создать свои меши через
    /// `add_cube_colored` напрямую и не пользоваться этим хелпером, если
    /// нужен другой цвет.
    pub fn add_car_demo_meshes(&mut self) -> (usize, usize) {
        let chassis_mesh = self.add_cube_colored(1.0, 0.55, 0.08, 0.08, 1.0);
        let wheel_mesh = self.add_cube_colored(1.0, 0.05, 0.05, 0.05, 1.0);
        (chassis_mesh, wheel_mesh)
    }

    /// ДОБАВЛЕНО (Задача #16 плана — физика и коллизии): проецирует
    /// текущее состояние каждого связанного физического тела
    /// (`PhysicsPlugin::get_body`) на позицию его визуальной ECS-сущности.
    /// Вызывается из `update()` СРАЗУ ПОСЛЕ `physics.update(dt, gravity)`
    /// — то есть уже после того, как плагин посчитал интегрирование и
    /// разрешил столкновения этого кадра, но ДО render_frame(), которая
    /// читает `Transform` для построения матриц мира (см. render_frame,
    /// проход по `mesh_instances`/сцене).
    ///
    /// Ничего не делает (тихо), если физика не инициализирована — в этом
    /// случае `physics_links` попросту пуст (см. `spawn_physics_sphere`,
    /// единственная точка добавления записей в него).
    pub(super) fn sync_physics_transforms(&mut self) {
        if self.physics.is_none() || self.physics_links.is_empty() {
            return;
        }
        for i in 0..self.physics_links.len() {
            let (body_id, entity) = self.physics_links[i];
            let Some(physics) = self.physics.as_ref() else { break };
            let body = physics.get_body(body_id);
            if let Some(t) = self.scene.transform_mut(entity) {
                t.position = body.position;
                t.rotation = quaternion_to_euler_zyx(body.orientation);
            }
        }
    }

    pub fn get_gpu_lights(&self) -> &[GPULight] {
        self.lights.as_ref().map(|l| l.get_gpu_lights()).unwrap_or(&[])
    }

    pub fn get_contacts(&self) -> &[PhysicsContact] {
        self.physics.as_ref().map(|p| p.get_contacts()).unwrap_or(&[])
    }

    /// ДОБАВЛЕНО (полноценная физика — запрос луча против сцены нуждается
    /// в РЕАЛЬНОЙ физической земле, не только визуальном меше): обёртка
    /// над `PhysicsPlugin::add_plane` — статичный полупространственный
    /// коллайдер (обычно пол/земля), см. `PlaneDesc` за подробностями (в
    /// т.ч. почему это годится только для бесконечного пола, не для стен
    /// ограниченного размера). `None`, если физика не инициализирована ИЛИ
    /// `normal` вырожден — тот же принцип, что и у прочих методов этого
    /// файла.
    pub fn add_physics_plane(&mut self, desc: &PlaneDesc) -> Option<i32> {
        self.physics.as_mut()?.add_plane(desc)
    }

    /// ДОБАВЛЕНО (разборка машины на детали — джойнты/constraint API):
    /// сырой доступ к `PhysicsPlugin::add_constraint` для случаев, не
    /// покрытых удобными `add_ball_joint`/`add_hinge_joint`/
    /// `add_fixed_joint` ниже (например JOINT_SLIDER, или когда нужен
    /// полный контроль над всеми полями `ConstraintDesc` сразу). `None`,
    /// если физика не инициализирована — тот же принцип "деградируем
    /// молча", что и у `add_physics_body`.
    pub fn add_constraint(&mut self, desc: &ConstraintDesc) -> Option<i32> {
        self.physics.as_mut()?.add_constraint(desc)
    }

    /// Шаровой шарнир — держит `body_a`/`body_b` в одной точке
    /// (`anchor_a`/`anchor_b` — смещения от центра каждого тела),
    /// вращение свободно по всем осям. Пример: буксировочный трос,
    /// подвеска на одной точке.
    pub fn add_ball_joint(
        &mut self,
        body_a: i32,
        body_b: i32,
        anchor_a: [f32; 3],
        anchor_b: [f32; 3],
        break_impulse_linear: f32,
    ) -> Option<i32> {
        self.add_constraint(&ConstraintDesc {
            body_a,
            body_b,
            joint_type: joint_type::BALL,
            anchor_a,
            anchor_b,
            break_impulse_linear,
            ..Default::default()
        })
    }

    /// Петля — точка крепления + вращение только вокруг `axis` (в
    /// мировых координатах). Пример: дверь, капот, крышка багажника —
    /// открываются вокруг оси петель, но не отрываются и не болтаются в
    /// стороны.
    pub fn add_hinge_joint(
        &mut self,
        body_a: i32,
        body_b: i32,
        anchor_a: [f32; 3],
        anchor_b: [f32; 3],
        axis: [f32; 3],
        break_impulse_linear: f32,
        break_impulse_angular: f32,
    ) -> Option<i32> {
        self.add_constraint(&ConstraintDesc {
            body_a,
            body_b,
            joint_type: joint_type::HINGE,
            anchor_a,
            anchor_b,
            axis_a: axis,
            axis_b: axis,
            break_impulse_linear,
            break_impulse_angular,
            ..Default::default()
        })
    }

    /// Жёсткая сварка/болтовое соединение — `body_a`/`body_b` двигаются
    /// как единое твёрдое тело, пока накопленная за физический шаг
    /// нагрузка не превысит `break_impulse_linear`/`break_impulse_angular`
    /// (`<= 0` — неразрушимо). Это и есть механика "деталь прикручена к
    /// машине, пока её не открутили/не оторвали силой" — снять деталь
    /// вручную означает вызвать `remove_constraint` на возвращённый
    /// handle, оторвать силой — довести накопленный импульс до порога и
    /// дождаться `is_broken`/`get_broken_constraints`.
    pub fn add_fixed_joint(
        &mut self,
        body_a: i32,
        body_b: i32,
        anchor_a: [f32; 3],
        anchor_b: [f32; 3],
        break_impulse_linear: f32,
        break_impulse_angular: f32,
    ) -> Option<i32> {
        self.add_constraint(&ConstraintDesc {
            body_a,
            body_b,
            joint_type: joint_type::FIXED,
            anchor_a,
            anchor_b,
            break_impulse_linear,
            break_impulse_angular,
            ..Default::default()
        })
    }

    /// Удаляет соединение (в т.ч. уже сломанное) по handle'у, возвращённому
    /// `add_constraint`/`add_ball_joint`/`add_hinge_joint`/`add_fixed_joint`.
    /// Ничего не делает, если физика не инициализирована.
    pub fn remove_constraint(&mut self, id: i32) {
        if let Some(p) = self.physics.as_mut() {
            p.remove_constraint(id);
        }
    }

    /// Текущее состояние соединения — `None`, если физика не
    /// инициализирована (в отличие от `PhysicsPlugin::get_constraint`,
    /// который в этом случае разыменовал бы null `instance`).
    pub fn get_constraint(&self, id: i32) -> Option<ConstraintInfo> {
        Some(self.physics.as_ref()?.get_constraint(id))
    }

    /// Handle'ы соединений, впервые сломавшихся на ПОСЛЕДНЕМ кадре физики
    /// — см. подробное объяснение у `PhysicsAPI::get_broken_constraints`
    /// в `plugin/physics_api.rs`. Игровой код опрашивает это раз за кадр
    /// (например сразу после `sync_physics_transforms` в `update()`),
    /// чтобы один раз проиграть звук/заспавнить обломок на каждую
    /// поломку, а не на каждый кадр, пока constraint остаётся сломанным.
    pub fn get_broken_constraints(&self) -> &[i32] {
        self.physics.as_ref().map(|p| p.get_broken_constraints()).unwrap_or(&[])
    }

    /// ДОБАВЛЕНО (Фаза 1 реальной физики): копит силу (Н, мировые
    /// координаты) в аккумулятор ДО следующего шага физики — зови КАЖДЫЙ
    /// кадр, пока сила должна действовать. No-op, если физика не
    /// инициализирована (тот же принцип деградации, что и у
    /// `remove_constraint` выше).
    pub fn apply_physics_force(&mut self, id: i32, force: [f32; 3]) {
        if let Some(p) = self.physics.as_mut() {
            p.apply_force(id, force);
        }
    }

    /// Мгновенно `v += impulse * inv_mass`.
    pub fn apply_physics_impulse(&mut self, id: i32, impulse: [f32; 3]) {
        if let Some(p) = self.physics.as_mut() {
            p.apply_impulse(id, impulse);
        }
    }

    /// Прямая перезапись линейной/угловой скорости тела (телепорт
    /// скорости).
    pub fn set_physics_velocity(&mut self, id: i32, linear: [f32; 3], angular: [f32; 3]) {
        if let Some(p) = self.physics.as_mut() {
            p.set_velocity(id, linear, angular);
        }
    }

    /// Прямая перезапись позиции/ориентации тела (телепорт) — скорость НЕ
    /// трогает.
    pub fn set_physics_transform(&mut self, id: i32, position: [f32; 3], orientation: [f32; 4]) {
        if let Some(p) = self.physics.as_mut() {
            p.set_transform(id, position, orientation);
        }
    }

    /// ДОБАВЛЕНО (реальная физика машины — подвеска): читает ТЕКУЩЕЕ
    /// состояние тела (позиция/скорость/ориентация/угловая скорость) —
    /// нужно каждый кадр ДО применения сил подвески, чтобы посчитать
    /// мировые точки крепления колёс и скорость в этих точках (см.
    /// `apply_physics_force_at_point`). `None`, если физика не
    /// инициализирована (тот же принцип деградации, что и у прочих
    /// методов этого файла) — сам плагин на несуществующий/статичный id
    /// отвечает "нулевым" телом, а не паникует, см. `default_abi_body` в
    /// alkash3d-inertial.
    pub fn get_physics_body(&self, id: i32) -> Option<PhysicsBody> {
        Some(self.physics.as_ref()?.get_body(id))
    }

    /// ДОБАВЛЕНО (реальная физика машины — подвеска): копит момент силы
    /// (Н·м) в аккумулятор ДО следующего шага физики.
    pub fn apply_physics_torque(&mut self, id: i32, torque: [f32; 3]) {
        if let Some(p) = self.physics.as_mut() {
            p.apply_torque(id, torque);
        }
    }

    /// Прикладывает силу В ТОЧКЕ `world_point` (мировые координаты), а не
    /// через центр масс — рождает и линейное ускорение, и момент, если
    /// точка приложения не совпадает с центром масс тела. Ключевая
    /// функция для честной подвески (сила пружины/демпфера на колесе).
    pub fn apply_physics_force_at_point(&mut self, id: i32, force: [f32; 3], world_point: [f32; 3]) {
        if let Some(p) = self.physics.as_mut() {
            p.apply_force_at_point(id, force, world_point);
        }
    }

    /// ДОБАВЛЕНО (полноценная физика — запрос луча против сцены): честный
    /// raycast против ВСЕХ живых физических тел и статичных плоскостей
    /// (см. `RaycastHit`/`kernels/raycast.f90` в `alkash3d-inertial`) —
    /// ключевая функция для подвески машины (`car_physics.rs`), которая
    /// раньше была вынуждена считать землю жёстко зашитой плоской высотой
    /// (`ground_y`), полностью игнорируя реальную физическую геометрию.
    /// `None`, если физика не инициализирована ИЛИ луч ничего не задел в
    /// пределах `max_dist` (тот же принцип деградации, что и у прочих
    /// методов этого файла) — вызывающий код сам решает, чем заменить
    /// отсутствие попадания (например, старым флэт-грунтом как fallback).
    /// `exclude_body` — handle тела, которое нужно пропустить (обычно
    /// собственное тело вызывающего — см. `PhysicsPlugin::raycast`).
    pub fn physics_raycast(&self, origin: [f32; 3], direction: [f32; 3], max_dist: f32, exclude_body: Option<i32>) -> Option<RaycastHit> {
        self.physics.as_ref()?.raycast(origin, direction, max_dist, exclude_body)
    }
}
