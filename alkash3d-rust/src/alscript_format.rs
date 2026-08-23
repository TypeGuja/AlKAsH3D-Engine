// alscript_format.rs - Scripting System
//
// ОБНОВЛЕНО (скриптинг — по прямому указанию пользователя): C# ВЫРЕЗАН
// из скриптинга целиком ("вырежи C# из скриптинга, будет жить на том что
// есть") — .NET/CoreCLR-хостинг требовал установки .NET SDK, чего
// пользователь делать не хочет. Движок живёт на трёх языках: Python
// (hot-reload), Lua (DLL), Native/C++/Rust (DLL). script_type=3
// зарезервирован (см. ниже) — при желании C# можно добавить обратно
// позже, но сейчас ни один код-путь его не реализует.
//   0 = Python — HOT-RELOAD, встроенный интерпретатор ПРЯМО в движке
//       (не DLL) — см. `engine::scripting_python` в engine/mod.rs.
//       .alscript хранит ПУТЬ к .py-файлу на диске (не байткод, см.
//       `register_python_script_path` ниже) — движок сам перечитывает
//       файл и следит за его mtime.
//   1 = Lua — компилируется/упаковывается в DLL, ТОТ ЖЕ C-ABI
//       ScriptingAPI, что и Native (см. `register_lua_script` ниже) —
//       DLL содержит embedded Lua-рантайм (mlua) и грузит .lua текстом
//       при старте. См. alkash3d-luascript.
//   2 = Native (C++/Rust) — DLL с ScriptingAPI, см. `register_native_script`
//       ниже и alkash3d-examplescript.
//   3 = зарезервировано, НЕ используется (был C# — вырезан из движка).
//   4 = Hybrid — зарезервировано, не используется.

use std::io::{Read, Write};
use std::collections::HashMap;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AlscriptHeader {
    pub magic: [u8; 8],          // "ALKSCRPT"
    pub version: u32,
    pub script_count: u32,
    pub native_binding_count: u32, // Количество нативных C-функций
    pub max_execution_time_ms: u32, // Таймаут для скриптов
    pub memory_limit_mb: u32,     // Лимит памяти для Python
    pub string_table_offset: u64,
    pub script_table_offset: u64,
    pub bytecode_offset: u64,
    pub native_table_offset: u64,
    pub type_registry_offset: u64,
    pub created_at: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ScriptDescriptor {
    pub name_id: u32,
    pub script_type: u32,         // 0=Python(hot-reload), 1=Lua(DLL), 2=Native(DLL), 3=резерв (был C#, вырезан), 4=Hybrid(резерв)
    pub compilation_mode: u32,    // 0=interpreted, 1=compiled, 2=JIT
    pub bytecode_offset: u64,
    pub bytecode_size: u64,
    /// Для `script_type == 0` (Python) и `script_type == 1` (Lua):
    /// ПЕРЕИСПОЛЬЗУЕТСЯ (см. ниже) как id строки в таблице `strings` —
    /// путь к исходному .py/.lua файлу на диске (относительно рабочей
    /// директории движка). Для Python это и есть основной способ найти
    /// скрипт (hot-reload перечитывает именно этот файл по mtime, байткод
    /// не хранится — исполняется исходник напрямую через встроенный
    /// интерпретатор). Для остальных script_type — как раньше,
    /// байтовое смещение исходника внутри файла (0, если не используется).
    pub source_offset: u64,
    /// Для Python/Lua при переиспользовании source_offset как id строки —
    /// не используется (длина строки уже хранится в самой строковой
    /// таблице), оставлено 0. Для остальных — как раньше, размер исходника.
    pub source_size: u64,
    pub dependencies_count: u32,
    pub dependency_ids_offset: u64,
    pub hot_reloadable: u32,
    pub run_on_thread: u32,       // -1=main thread, 0+=worker thread
    pub priority: i32,
    pub owner_entity_id: u64,     // Какому объекту принадлежит (упакованный
                                   // EntityId — см. scene::EntityId: (index
                                   // as u64) << 32 | generation as u64)
    /// ДОБАВЛЕНО (скриптинг, этап 1 — нативные C++/Rust плагины);
    /// ОБНОВЛЕНО (вторая волна — Lua тоже стал DLL-плагином): для
    /// `script_type == 1` (Lua) и `script_type == 2` (Native) — id строки
    /// в таблице `strings`, содержащей путь к DLL, реализующей
    /// `ScriptingAPI` (см. `plugin/scripting_api.rs`). Для Lua эта DLL —
    /// универсальный рантайм-плагин (см. alkash3d-luascript), а не
    /// скрипт-специфичная сборка — конкретный .lua-файл указывается через
    /// `source_offset`/`source_size` (путь к .lua как исходник, см.
    /// `register_lua_script`), точно так же, как раньше уже
    /// предполагалось для отладочного исходника. Для остальных
    /// script_type не используется (0xFFFFFFFF — "нет пути", тот же приём
    /// "невалидного id", что и `module_name_id`/`callback_id` в
    /// `NativeBinding` ниже). В КОНЦЕ структуры — формат ещё не имел
    /// сериализации на диск (не было ни одного .alscript-файла), так что
    /// добавление поля здесь НЕ ломает обратную совместимость ни с чем
    /// существующим.
    pub native_dll_path_id: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NativeBinding {
    pub function_name_id: u32,
    pub module_name_id: u32,
    pub function_pointer: u64,    // Адрес нативной функции
    pub arg_count: u32,
    pub arg_types: u64,           // Битовая маска типов
    pub return_type: u32,
    pub is_async: u32,
    pub callback_id: u32,         // Для обратного вызова из нативного кода
}

#[repr(C)]
pub struct TypeRegistry {
    pub class_name_id: u32,
    pub native_struct_size: u32,
    pub field_count: u32,
    pub fields: *mut FieldDescriptor, // Динамический массив
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FieldDescriptor {
    pub name_id: u32,
    pub offset: u32,
    pub field_type: u32,          // int, float, string, object, array
    pub array_element_type: u32,
    pub is_property: u32,         // Имеет getter/setter
    pub getter_func_id: u32,
    pub setter_func_id: u32,
}

#[repr(C)]
pub struct ScriptExecutionContext {
    pub entity_id: u64,
    pub delta_time: f32,
    pub frame_number: u64,
    pub input_data: [f32; 16],    // Входные параметры
    pub output_data: [f32; 16],   // Выходные данные
    pub custom_data_offset: u64,
}

pub struct AlscriptFile {
    pub header: AlscriptHeader,
    pub strings: Vec<String>,
    pub scripts: Vec<ScriptDescriptor>,
    pub native_bindings: Vec<NativeBinding>,
    pub type_registry: Vec<TypeRegistry>,
    pub python_bytecode: Vec<u8>,
    pub native_libs: Vec<u8>,     // DLL/SO файлы для JIT
}

impl AlscriptFile {
    pub fn new() -> Self {
        Self {
            header: AlscriptHeader {
                magic: *b"ALKSCRPT",
                version: 1,
                script_count: 0,
                native_binding_count: 0,
                max_execution_time_ms: 16, // Максимум 16мс на кадр
                memory_limit_mb: 256,
                string_table_offset: 0,
                script_table_offset: 0,
                bytecode_offset: 0,
                native_table_offset: 0,
                type_registry_offset: 0,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap().as_secs(),
            },
            strings: Vec::new(),
            scripts: Vec::new(),
            native_bindings: Vec::new(),
            type_registry: Vec::new(),
            python_bytecode: Vec::new(),
            native_libs: Vec::new(),
        }
    }

    /// УСТАРЕЛО для реального pipeline (сохранено ради обратной
    /// совместимости с `create_vehicle_ai_script` ниже, где Python-код
    /// хранится инлайн как строка, а не как файл на диске):
    /// byte-код здесь — это по-прежнему заглушка
    /// (`compile_python_to_bytecode`), а движок в РЕАЛЬНОСТИ исполняет
    /// Python только через `register_python_script_path` (путь к .py на
    /// диске, hot-reload встроенным интерпретатором — см.
    /// `engine::scripting_python`). Для нового кода использовать
    /// `register_python_script_path`.
    pub fn register_python_script(&mut self, name: &str, source: &str) -> u32 {
        let script_id = self.scripts.len() as u32;
        let name_id = self.add_string(name);

        // Храним и байткод, и исходник
        let bytecode = self.compile_python_to_bytecode(source);
        let bc_offset = self.python_bytecode.len() as u64;
        self.python_bytecode.extend_from_slice(&bytecode);

        let source_offset = 0; // Для простоты, в реальности нужно добавлять в секцию
        let source_size = source.len() as u64;

        self.scripts.push(ScriptDescriptor {
            name_id,
            script_type: 0,      // Python
            compilation_mode: 1, // Compiled
            bytecode_offset: bc_offset,
            bytecode_size: bytecode.len() as u64,
            source_offset,
            source_size,
            dependencies_count: 0,
            dependency_ids_offset: 0,
            hot_reloadable: 1,
            run_on_thread: 0,
            priority: 0,
            owner_entity_id: 0,
            native_dll_path_id: 0xFFFFFFFF,
        });

        self.header.script_count += 1;
        script_id
    }

    fn compile_python_to_bytecode(&self, source: &str) -> Vec<u8> {
        // В реальной имплементации здесь будет вызов Python C API
        // Py_CompileString() для получения байткода
        // Пока заглушка
        source.as_bytes().to_vec()
    }

    /// ДОБАВЛЕНО (скриптинг, этап 1 — нативные C++/Rust плагины):
    /// регистрирует скрипт, чья логика полностью живёт в отдельной DLL
    /// (см. `plugin::ScriptingPlugin`/`plugin::scripting_api::ScriptingAPI`)
    /// — в отличие от `register_python_script`, здесь НЕТ байткода: вместо
    /// него в строковую таблицу кладётся путь к DLL
    /// (`ScriptDescriptor::native_dll_path_id`), а движок при загрузке
    /// сцены сам решает, грузить ли эту DLL заново или переиспользовать
    /// уже загруженную (см. `AlkashEngine::load_native_script` в
    /// engine/mod.rs — одна DLL может обслуживать несколько сущностей).
    ///
    /// `owner_entity_id` — уже упакованный EntityId (см. подробности у
    /// одноимённого поля `ScriptDescriptor` выше); `0` — валидное значение
    /// ("ещё не привязан ни к какой сущности", присваивается настоящий id
    /// уже на движковой стороне при фактическом спавне).
    pub fn register_native_script(&mut self, name: &str, dll_path: &str, owner_entity_id: u64) -> u32 {
        let script_id = self.scripts.len() as u32;
        let name_id = self.add_string(name);
        let dll_path_id = self.add_string(dll_path);

        self.scripts.push(ScriptDescriptor {
            name_id,
            script_type: 2,       // Native
            compilation_mode: 1,  // Compiled — DLL уже собрана заранее
            bytecode_offset: 0,
            bytecode_size: 0,
            source_offset: 0,
            source_size: 0,
            dependencies_count: 0,
            dependency_ids_offset: 0,
            // ИЗМЕНЕНО: нативные C++/Rust-скрипты по просьбе
            // пользователя НЕ поддерживают hot-reload на этом этапе —
            // правка требует перекомпиляции DLL, а не горячей подмены
            // (в отличие от Python/Lua, которые придут позже как
            // отдельная, встроенная-интерпретатор архитектура). `0`, а не
            // `1`, чтобы это было видно из самих данных файла, а не
            // только из документации.
            hot_reloadable: 0,
            run_on_thread: 0,
            priority: 0,
            owner_entity_id,
            native_dll_path_id: dll_path_id,
        });

        self.header.script_count += 1;
        script_id
    }

    /// ДОБАВЛЕНО (скриптинг, вторая волна — Lua как DLL-плагин):
    /// регистрирует Lua-скрипт. В отличие от `register_native_script`,
    /// `dll_path` здесь — путь к УНИВЕРСАЛЬНОМУ Lua-рантайм-плагину
    /// (alkash3d-luascript, одна и та же DLL для всех Lua-скриптов
    /// проекта — она сама грузит .lua текстом при `create_script`), а
    /// сам .lua-файл передаётся отдельно через `script_source_path` —
    /// сохраняется в строковой таблице и кладётся в переиспользуемое поле
    /// `source_offset` (см. комментарий у `ScriptDescriptor` выше).
    pub fn register_lua_script(&mut self, name: &str, dll_path: &str, script_source_path: &str, owner_entity_id: u64) -> u32 {
        let script_id = self.scripts.len() as u32;
        let name_id = self.add_string(name);
        let dll_path_id = self.add_string(dll_path);
        let source_path_id = self.add_string(script_source_path);

        self.scripts.push(ScriptDescriptor {
            name_id,
            script_type: 1,        // Lua
            compilation_mode: 0,   // interpreted — mlua исполняет .lua текст напрямую
            bytecode_offset: 0,
            bytecode_size: 0,
            source_offset: source_path_id as u64,
            source_size: 0,
            dependencies_count: 0,
            dependency_ids_offset: 0,
            // Технически возможен hot-reload (перечитать .lua и
            // пересоздать состояние Lua-скрипта), но на этом этапе НЕ
            // реализован в alkash3d-luascript (только загрузка при
            // create_script) — 0, чтобы честно отражать текущее
            // поведение, а не декларировать нереализованное.
            hot_reloadable: 0,
            run_on_thread: 0,
            priority: 0,
            owner_entity_id,
            native_dll_path_id: dll_path_id,
        });

        self.header.script_count += 1;
        script_id
    }

    /// ДОБАВЛЕНО (скриптинг, вторая волна — Python как hot-reload):
    /// регистрирует Python-скрипт, живущий как путь к .py-файлу на диске
    /// — в отличие от старого `register_python_script` (байткод-заглушка,
    /// сохранён ниже для обратной совместимости с уже существующим
    /// `create_vehicle_ai_script`), здесь НЕТ ни байткода, ни DLL: движок
    /// сам, БЕЗ отдельного ScriptingAPI-плагина, перечитывает файл по
    /// `script_source_path` и исполняет его через встроенный интерпретатор
    /// (см. `engine::scripting_python` в engine/mod.rs) — `hot_reloadable`
    /// всегда `1`, это и есть весь смысл Python-варианта.
    pub fn register_python_script_path(&mut self, name: &str, script_source_path: &str, owner_entity_id: u64) -> u32 {
        let script_id = self.scripts.len() as u32;
        let name_id = self.add_string(name);
        let source_path_id = self.add_string(script_source_path);

        self.scripts.push(ScriptDescriptor {
            name_id,
            script_type: 0,       // Python
            compilation_mode: 0,  // interpreted — hot-reload, байткод не хранится
            bytecode_offset: 0,
            bytecode_size: 0,
            source_offset: source_path_id as u64,
            source_size: 0,
            dependencies_count: 0,
            dependency_ids_offset: 0,
            hot_reloadable: 1,
            run_on_thread: 0,
            priority: 0,
            owner_entity_id,
            native_dll_path_id: 0xFFFFFFFF,
        });

        self.header.script_count += 1;
        script_id
    }

    pub fn register_native_function(&mut self, name: &str, func_ptr: u64) -> u32 {
        let func_id = self.native_bindings.len() as u32;
        let name_id = self.add_string(name);

        self.native_bindings.push(NativeBinding {
            function_name_id: name_id,
            module_name_id: 0xFFFFFFFF,
            function_pointer: func_ptr,
            arg_count: 0,
            arg_types: 0,
            return_type: 0,
            is_async: 0,
            callback_id: 0xFFFFFFFF,
        });

        self.header.native_binding_count += 1;
        func_id
    }

    pub fn create_vehicle_ai_script() -> Self {
        let mut script = AlscriptFile::new();

        let ai_source = r#"
import alkash3d as a3d
import math

class VehicleAI:
    def __init__(self, vehicle):
        self.vehicle = vehicle
        self.target_speed = 0.0
        self.target_steering = 0.0

    def update(self, dt, world_state):
        # Нативная проверка коллизий
        front_clear = a3d.raycast(self.vehicle.position,
                                   self.vehicle.forward * 50.0)

        if front_clear.distance < 10.0:
            self.emergency_brake()
        else:
            self.follow_path(world_state.navigation_path)

    @a3d.native_call  # Помечаем для JIT-компиляции в нативный код
    def follow_path(self, path):
        # Тяжёлые вычисления, исполняются нативно
        next_waypoint = path.get_next()
        self.target_speed = next_waypoint.speed_limit
        self.target_steering = self.calculate_steering(next_waypoint)

    def emergency_brake(self):
        self.vehicle.brake = 1.0
        self.vehicle.throttle = 0.0
"#;

        script.register_python_script("VehicleAI", ai_source);
        script
    }

    // ИЗМЕНЕНО (дедупликация строк — тот же приём, что и в .alsnd/.alworld/
    // .altex): раньше `add_string` слепо пушило новую строку на каждый
    // вызов — при регистрации нескольких скриптов одной и той же DLL
    // (например одна "vehicle_ai.dll" на 20 машин) путь к DLL дублировался
    // бы в строковой таблице 20 раз.
    fn add_string(&mut self, s: &str) -> u32 {
        if let Some(pos) = self.strings.iter().position(|existing| existing == s) {
            return pos as u32;
        }
        self.strings.push(s.to_string());
        (self.strings.len() - 1) as u32
    }

    pub fn get_string(&self, id: u32) -> &str {
        self.strings.get(id as usize).map(|s| s.as_str()).unwrap_or("")
    }

    // =========================================================================
    // ДОБАВЛЕНО (скриптинг, этап 1 — save()/load() для .alscript): раньше
    // формат существовал ТОЛЬКО в памяти (никакого способа сохранить сцену
    // со скриптами на диск и загрузить обратно не было вообще — в отличие
    // от всех остальных .al*-форматов движка). Схема — тот же паттерн, что
    // уже используется в .alsnd/.alworld: header (с offset'ами на каждую
    // таблицу) -> string table (count + [len(u32)+bytes]) -> scripts ->
    // native_bindings -> python_bytecode (сырой блоб) -> native_libs
    // (сырой блоб), каждая табличная секция — count(u32) + POD-массив.
    //
    // `type_registry` СОЗНАТЕЛЬНО НЕ сериализуется: `TypeRegistry.fields`
    // — сырой `*mut FieldDescriptor`, не POD и не имеет смысла как байты
    // на диске (адрес в памяти конкретного процесса). Для нативных
    // (DLL) скриптов, единственного вида скриптов, реализованного на этом
    // этапе, отражение полей через `TypeRegistry`/`FieldDescriptor` не
    // нужно — вся типизация идёт через C-ABI `ScriptingAPI` напрямую (см.
    // plugin/scripting_api.rs). Секция останется пустой (0 записей) до
    // тех пор, пока Python/Lua-скриптинг (этапы 2-3) не потребует
    // настоящей рефлексии полей движка изнутри интерпретатора — тогда эту
    // часть нужно будет спроектировать заново (raw pointer недостаточен и
    // для будущей сериализации тоже, а не только сейчас).
    // =========================================================================
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;

        let mut strings_data = Vec::new();
        strings_data.extend_from_slice(&(self.strings.len() as u32).to_le_bytes());
        for s in &self.strings {
            strings_data.extend_from_slice(&(s.len() as u32).to_le_bytes());
            strings_data.extend_from_slice(s.as_bytes());
        }

        let mut scripts_data = Vec::new();
        scripts_data.extend_from_slice(&(self.scripts.len() as u32).to_le_bytes());
        for script in &self.scripts {
            scripts_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(script as *const ScriptDescriptor as *const u8, std::mem::size_of::<ScriptDescriptor>())
            });
        }

        let mut native_bindings_data = Vec::new();
        native_bindings_data.extend_from_slice(&(self.native_bindings.len() as u32).to_le_bytes());
        for binding in &self.native_bindings {
            native_bindings_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(binding as *const NativeBinding as *const u8, std::mem::size_of::<NativeBinding>())
            });
        }

        // type_registry: пустая секция (count=0) — см. комментарий выше.
        let mut type_registry_data = Vec::new();
        type_registry_data.extend_from_slice(&0u32.to_le_bytes());

        let mut bytecode_data = Vec::new();
        bytecode_data.extend_from_slice(&(self.python_bytecode.len() as u32).to_le_bytes());
        bytecode_data.extend_from_slice(&self.python_bytecode);

        let mut native_libs_data = Vec::new();
        native_libs_data.extend_from_slice(&(self.native_libs.len() as u32).to_le_bytes());
        native_libs_data.extend_from_slice(&self.native_libs);

        let header_size = std::mem::size_of::<AlscriptHeader>() as u64;
        let string_table_offset = header_size;
        let script_table_offset = string_table_offset + strings_data.len() as u64;
        let native_table_offset = script_table_offset + scripts_data.len() as u64;
        let type_registry_offset = native_table_offset + native_bindings_data.len() as u64;
        let bytecode_offset = type_registry_offset + type_registry_data.len() as u64;
        let native_libs_offset = bytecode_offset + bytecode_data.len() as u64;

        let header = AlscriptHeader {
            magic: self.header.magic,
            version: self.header.version,
            script_count: self.scripts.len() as u32,
            native_binding_count: self.native_bindings.len() as u32,
            max_execution_time_ms: self.header.max_execution_time_ms,
            memory_limit_mb: self.header.memory_limit_mb,
            string_table_offset,
            script_table_offset,
            bytecode_offset,
            native_table_offset,
            type_registry_offset,
            created_at: self.header.created_at,
        };

        file.write_all(unsafe {
            std::slice::from_raw_parts(&header as *const AlscriptHeader as *const u8, std::mem::size_of::<AlscriptHeader>())
        })?;
        file.write_all(&strings_data)?;
        file.write_all(&scripts_data)?;
        file.write_all(&native_bindings_data)?;
        file.write_all(&type_registry_data)?;
        file.write_all(&bytecode_data)?;
        // native_libs_offset вычислен, но само поле в AlscriptHeader под
        // него не заведено (структура заголовка не менялась — новое поле
        // добавлено только в ScriptDescriptor, см. native_dll_path_id).
        // native_libs пишется СРАЗУ после bytecode_data — фиксированное
        // относительное расположение, читатель (load() ниже) вычисляет
        // его смещение так же, через bytecode_offset + фактический размер
        // прочитанного блоба, а не через отдельное поле заголовка.
        let _ = native_libs_offset;
        file.write_all(&native_libs_data)?;

        Ok(())
    }

    pub fn load(path: &str) -> std::io::Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let header_size = std::mem::size_of::<AlscriptHeader>();
        if buf.len() < header_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "alscript: файл короче заголовка AlscriptHeader",
            ));
        }

        // SAFETY: AlscriptHeader — #[repr(C)], POD (только числа и
        // [u8;8]), длина буфера уже проверена выше.
        let header: AlscriptHeader = unsafe {
            std::ptr::read_unaligned(buf.as_ptr() as *const AlscriptHeader)
        };

        if &header.magic != b"ALKSCRPT" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("alscript: неверная сигнатура {:?}, ожидалось ALKSCRPT", header.magic),
            ));
        }
        if header.version != 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("alscript: неподдерживаемая версия формата {}", header.version),
            ));
        }

        let read_at = |offset: u64, size: usize, what: &str| -> std::io::Result<&[u8]> {
            let start = offset as usize;
            let end = start.checked_add(size).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("alscript: переполнение при вычислении конца блока {}", what),
            ))?;
            buf.get(start..end).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("alscript: блок {} выходит за пределы файла (offset={}, size={}, file_len={})", what, offset, size, buf.len()),
            ))
        };

        // Строковая таблица: count(u32) + N раз [len(u32) + байты строки].
        let strings_start = header.string_table_offset as usize;
        if strings_start > buf.len() || strings_start + 4 > buf.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "alscript: string_table_offset выходит за пределы файла",
            ));
        }
        let string_count = u32::from_le_bytes(buf[strings_start..strings_start + 4].try_into().unwrap()) as usize;
        let mut cursor = strings_start + 4;
        let mut strings = Vec::with_capacity(string_count);
        for _ in 0..string_count {
            let len_bytes = read_at(cursor as u64, 4, "string length")?;
            let len = u32::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
            cursor += 4;
            let str_bytes = read_at(cursor as u64, len, "string data")?;
            strings.push(String::from_utf8_lossy(str_bytes).into_owned());
            cursor += len;
        }

        // Небольшой локальный хелпер, читающий "count(u32) + POD-массив T"
        // блок по заданному offset'у — та же схема, что в .alsnd.
        fn read_table<T: Copy>(buf: &[u8], offset: u64, what: &str) -> std::io::Result<Vec<T>> {
            let start = offset as usize;
            if start + 4 > buf.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!("alscript: {}_offset выходит за пределы файла", what),
                ));
            }
            let count = u32::from_le_bytes(buf[start..start + 4].try_into().unwrap()) as usize;
            let item_size = std::mem::size_of::<T>();
            let data_start = start + 4;
            let data_end = data_start.checked_add(count * item_size).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("alscript: переполнение при вычислении размера таблицы {}", what),
            ))?;
            let bytes = buf.get(data_start..data_end).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("alscript: таблица {} выходит за пределы файла (offset={}, count={}, file_len={})", what, offset, count, buf.len()),
            ))?;
            let mut items = Vec::with_capacity(count);
            for i in 0..count {
                let s = i * item_size;
                let item: T = unsafe { std::ptr::read_unaligned(bytes[s..s + item_size].as_ptr() as *const T) };
                items.push(item);
            }
            Ok(items)
        }

        let scripts: Vec<ScriptDescriptor> = read_table(&buf, header.script_table_offset, "script_table")?;
        let native_bindings: Vec<NativeBinding> = read_table(&buf, header.native_table_offset, "native_table")?;
        // type_registry намеренно не читается — секция на диске всегда
        // пустая (см. комментарий у save()), Vec::new() ниже отражает это
        // напрямую, без бессмысленного чтения нулевой таблицы.

        // Байткод и native_libs — сырые блобы (count(u32) байт + сами
        // байты), а не POD-таблицы фиксированного размера элемента.
        let read_blob = |offset: u64, what: &str| -> std::io::Result<(Vec<u8>, u64)> {
            let start = offset as usize;
            if start + 4 > buf.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!("alscript: {} offset выходит за пределы файла", what),
                ));
            }
            let len = u32::from_le_bytes(buf[start..start + 4].try_into().unwrap()) as usize;
            let data = read_at((start + 4) as u64, len, what)?;
            Ok((data.to_vec(), (start + 4 + len) as u64))
        };

        let (python_bytecode, after_bytecode) = read_blob(header.bytecode_offset, "python_bytecode")?;
        // native_libs хранится СРАЗУ после bytecode-блоба (см. комментарий
        // в save() про отсутствие отдельного поля в заголовке под него) —
        // читаем со смещения, вычисленного по факту прочитанного размера
        // bytecode-блоба, а не по отдельному offset-полю.
        let (native_libs, _) = read_blob(after_bytecode, "native_libs")?;

        Ok(Self {
            header,
            strings,
            scripts,
            native_bindings,
            type_registry: Vec::new(),
            python_bytecode,
            native_libs,
        })
    }
}
