// src/engine/scripting_python.rs
//
// ДОБАВЛЕНО (скриптинг, вторая волна — Python как hot-reload): в отличие
// от Native/Lua (см. plugin/scripting_api.rs, alkash3d-examplescript,
// alkash3d-luascript — компилируются в отдельную DLL заранее), Python
// работает ЧЕРЕЗ ВСТРОЕННЫЙ интерпретатор ПРЯМО В ДВИЖКЕ, без DLL вообще
// — так и было решено пользователем ("а питон в hot load").
//
// Выбор интерпретатора: `rustpython-vm` (чистый Rust), а НЕ `pyo3`.
// `pyo3` без готового embeddable Python на машине требует системной
// установки CPython (dev-заголовки + python3XX.dll рядом с .exe) — прямое
// противоречие цели "работает на 10-летнем минимальном железе из
// коробки", той же логике, что уже применена к XAudio2 в audio.rs
// (встроенный компонент ОС, БЕЗ внешнего redist). `rustpython-vm`
// линкуется статически в сам alkash3d_rs.exe, как и всё остальное —
// никаких дополнительных файлов/установок на машине игрока не требуется.
//
// Hot-reload: движок НЕ хранит скомпилированный байткод на диске (в
// отличие от `register_python_script` в alscript_format.rs, оставленного
// только для обратной совместимости) — каждый `PythonScriptRuntime`
// хранит путь к .py-файлу и mtime последней загрузки; `update()` (per
// frame, до фактического вызова Python-функции) сверяет текущий mtime
// файла на диске и, если он изменился, ПОЛНОСТЬЮ пересоздаёт Python-scope
// заново (переисполняет файл с нуля — простая и надёжная модель, скрипт
// сам отвечает за пересоздание своего состояния, никакого "умного" сохранения
// переменных между перезагрузками не делается).

use rustpython_vm::{self as pvm, PyObjectRef, Interpreter};
use rustpython_vm::function::FuncArgs;
use std::time::SystemTime;

// ДОБАВЛЕНО (оптимизация — жалоба пользователя на лаги/периодические
// секундные просадки FPS при движении камеры): `check_hot_reload()`
// раньше вызывала `std::fs::metadata()` (реальный дисковый syscall —
// `GetFileAttributesEx` на Windows) БЕЗУСЛОВНО каждый кадр, для каждого
// прикреплённого Python-скрипта, без какого-либо ограничения частоты —
// в отличие от аналогичной проверки в world streaming
// (`WORLD_STREAMING_INTERVAL_FRAMES`, engine/mod.rs), где такой троттлинг
// уже есть. `stat()`/`GetFileAttributesEx` обычно занимает микросекунды,
// НО на Windows под антивирусом/Defender файловые syscall'ы иногда
// перехватываются и блокируются на десятки-сотни миллисекунд —
// нерегулярно, не на каждый вызов, а периодически, когда сработал
// фоновый скан. Именно такой профиль ("не в каждом кадре, а иногда, но
// зато на заметное время") соответствует наблюдаемым секундным просадкам
// FPS в логе пользователя. Троттлим проверку до раза в
// `HOT_RELOAD_CHECK_INTERVAL_FRAMES` кадров — этого более чем достаточно
// для отзывчивого hot-reload (доля секунды при любом разумном FPS), но
// убирает syscall из подавляющего большинства кадров.
const HOT_RELOAD_CHECK_INTERVAL_FRAMES: u32 = 20;

// Python-интерпретатор — ПОТОКОЛОКАЛЬНЫЙ (`thread_local!`, не общий
// `static`/`OnceLock`): `rustpython_vm::Interpreter` внутри держит
// нарочно не-`Sync`/не-`Send` состояние (стек фреймов, RefCell'ы), не
// предназначенное для расшаривания между потоками — попытка положить
// его в `static` не компилируется (см. проверку через отдельный пробник
// перед интеграцией). Это не проблема: движок вызывает Python-скрипты
// только из главного потока обновления сцены (см.
// `AlkashEngine::update_python_scripts`), так что один интерпретатор на
// главный поток — ровно то, что нужно. Создание — дорогая операция
// (инициализация стандартной библиотеки), поэтому создаётся ОДИН раз на
// поток и переиспользуется для ВСЕХ .py-скриптов; изоляция между разными
// .py-файлами обеспечивается отдельной `Scope`/globals-словарём на
// каждый `PythonScriptRuntime`, а не отдельным интерпретатором.
thread_local! {
    static INTERP: Interpreter = pvm::Interpreter::with_init(Default::default(), |vm| {
        vm.add_native_modules(rustpython_stdlib::get_module_inits());
    });
}

fn with_interp<R>(f: impl FnOnce(&Interpreter) -> R) -> R {
    INTERP.with(|i| f(i))
}

/// Одно прикрепление Python-скрипта к сущности — аналог `LuaInstance` в
/// alkash3d-luascript, но живущее прямо в движке, без DLL.
pub struct PythonScriptRuntime {
    pub entity: crate::scene::EntityId,
    source_path: String,
    last_mtime: Option<SystemTime>,
    /// `PyObjectRef` (Scope.globals как объект) хранится через
    /// `Interpreter::enter` при каждом использовании — сам `Scope` из
    /// rustpython-vm не является `Send`-совместимым для хранения "как
    /// есть" вне `enter()`, поэтому здесь хранится только dict глобалей,
    /// пересоздаваемый вместе с остальным состоянием при hot-reload.
    globals_dict: Option<PyObjectRef>,
    has_update: bool,
    has_on_event: bool,
    load_error: Option<String>,
    /// ДОБАВЛЕНО (см. `HOT_RELOAD_CHECK_INTERVAL_FRAMES` выше): считает
    /// кадры с момента последней реальной проверки mtime файла на диске —
    /// `check_hot_reload()` пропускает проверку, пока счётчик не достигнет
    /// интервала, вместо проверки на КАЖДОМ кадре.
    frames_since_hot_reload_check: u32,
}

impl PythonScriptRuntime {
    /// Создаёт новое прикрепление и сразу выполняет первую загрузку —
    /// зеркалит `LuaInstance`/`PluginState::create` в alkash3d-luascript:
    /// ошибка загрузки не паникует, а помечается в `load_error` (виден
    /// через `has_error()`/`last_error()`) — движок продолжает работать
    /// дальше, просто этот скрипт бездействует, пока файл не будет
    /// исправлен (что hot-reload подхватит на следующем кадре сам).
    pub fn new(entity: crate::scene::EntityId, source_path: &str) -> Self {
        let mut runtime = Self {
            entity,
            source_path: source_path.to_string(),
            last_mtime: None,
            globals_dict: None,
            has_update: false,
            has_on_event: false,
            load_error: None,
            frames_since_hot_reload_check: 0,
        };
        runtime.reload();
        runtime
    }

    pub fn has_error(&self) -> bool {
        self.load_error.is_some()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    fn file_mtime(&self) -> Option<SystemTime> {
        std::fs::metadata(&self.source_path).ok()?.modified().ok()
    }

    /// Полная перезагрузка: перечитывает файл, компилирует и исполняет
    /// заново — старые глобальные переменные скрипта теряются (см.
    /// пояснение в шапке файла про "простую и надёжную модель").
    fn reload(&mut self) {
        let source = match std::fs::read_to_string(&self.source_path) {
            Ok(s) => s,
            Err(e) => {
                self.load_error = Some(format!("не удалось прочитать '{}': {}", self.source_path, e));
                self.globals_dict = None;
                return;
            }
        };

        let outcome: Result<(PyObjectRef, bool, bool), String> = with_interp(|interp| interp.enter(|vm| {
            let scope = vm.new_scope_with_builtins();
            let code_obj = vm
                .compile(&source, pvm::compiler::Mode::Exec, self.source_path.clone())
                .map_err(|e| format!("{:?}", e))?;
            vm.run_code_obj(code_obj, scope.clone())
                .map_err(|e| format_py_exception(vm, &e))?;

            let has_update = scope.globals.get_item("update", vm).is_ok();
            let has_on_event = scope.globals.get_item("on_event", vm).is_ok();
            Ok((scope.globals.into(), has_update, has_on_event))
        }));

        match outcome {
            Ok((globals, has_update, has_on_event)) => {
                self.globals_dict = Some(globals);
                self.has_update = has_update;
                self.has_on_event = has_on_event;
                self.load_error = None;
                self.last_mtime = self.file_mtime();
            }
            Err(e) => {
                eprintln!("[python-script] ошибка загрузки '{}': {}", self.source_path, e);
                self.load_error = Some(e);
                self.globals_dict = None;
            }
        }
    }

    /// Проверяет mtime файла и перезагружает при изменении — вызывается
    /// движком ПЕРЕД `call_update` каждый кадр (см.
    /// `AlkashEngine::update_python_scripts` в engine/mod.rs).
    ///
    /// ИЗМЕНЕНО (оптимизация — см. `HOT_RELOAD_CHECK_INTERVAL_FRAMES`
    /// выше): реальный дисковый `file_mtime()`-вызов теперь происходит не
    /// на каждый кадр, а раз в `HOT_RELOAD_CHECK_INTERVAL_FRAMES` кадров —
    /// тот же троттлинг-паттерн, что уже используется в world streaming
    /// (`update_world_streaming`/`WORLD_STREAMING_INTERVAL_FRAMES`,
    /// engine/mod.rs).
    fn check_hot_reload(&mut self) {
        self.frames_since_hot_reload_check += 1;
        if self.frames_since_hot_reload_check < HOT_RELOAD_CHECK_INTERVAL_FRAMES {
            return;
        }
        self.frames_since_hot_reload_check = 0;

        let current = self.file_mtime();
        if current.is_some() && current != self.last_mtime {
            self.reload();
        }
    }

    /// Вызывает `update(dt, x,y,z, rx,ry,rz) -> (x,y,z,changed)`, если
    /// скрипт её определяет — тот же числовой протокол, что и у Lua (см.
    /// alkash3d-luascript/src/lib.rs), специально для единообразия между
    /// всеми языками скриптинга движка.
    pub fn call_update(
        &mut self,
        dt: f32,
        position: [f32; 3],
        rotation: [f32; 3],
    ) -> Option<[f32; 3]> {
        self.check_hot_reload();
        if !self.has_update {
            return None;
        }
        let Some(globals) = self.globals_dict.clone() else {
            return None;
        };

        let result: Result<Option<[f32; 3]>, String> = with_interp(|interp| interp.enter(|vm| {
            let dict = globals.downcast::<pvm::builtins::PyDict>().map_err(|_| "globals не dict".to_string())?;
            let func = dict.get_item("update", vm).map_err(|e| format_py_exception(vm, &e))?;

            let args = FuncArgs::from(vec![
                vm.ctx.new_float(dt as f64).into(),
                vm.ctx.new_float(position[0] as f64).into(),
                vm.ctx.new_float(position[1] as f64).into(),
                vm.ctx.new_float(position[2] as f64).into(),
                vm.ctx.new_float(rotation[0] as f64).into(),
                vm.ctx.new_float(rotation[1] as f64).into(),
                vm.ctx.new_float(rotation[2] as f64).into(),
            ]);

            let result = func.call(args, vm).map_err(|e| format_py_exception(vm, &e))?;
            let tuple = result
                .downcast::<pvm::builtins::PyTuple>()
                .map_err(|_| "update() должна вернуть кортеж (x,y,z,changed)".to_string())?;
            let items = tuple.as_slice();
            if items.len() != 4 {
                return Err(format!("update() вернула кортеж длины {}, ожидалось 4", items.len()));
            }
            let changed: bool = items[3].clone().try_into_value::<i64>(vm).map_err(|e| format_py_exception(vm, &e))? != 0;
            if !changed {
                return Ok(None);
            }
            let x: f64 = items[0].clone().try_into_value(vm).map_err(|e| format_py_exception(vm, &e))?;
            let y: f64 = items[1].clone().try_into_value(vm).map_err(|e| format_py_exception(vm, &e))?;
            let z: f64 = items[2].clone().try_into_value(vm).map_err(|e| format_py_exception(vm, &e))?;
            Ok(Some([x as f32, y as f32, z as f32]))
        }));

        match result {
            Ok(pos) => pos,
            Err(e) => {
                eprintln!("[python-script] ошибка в update() ('{}'): {}", self.source_path, e);
                self.load_error = Some(e);
                None
            }
        }
    }

    /// Вызывает `on_event(event_type, data0, data1, data2, data3)`, если
    /// определена — тот же протокол, что и `dispatch_event`/`on_event` у
    /// Native/Lua (см. `ScriptEvent` в plugin/scripting_api.rs).
    pub fn call_on_event(&mut self, event_type: u32, data: [f32; 4]) {
        if !self.has_on_event {
            return;
        }
        let Some(globals) = self.globals_dict.clone() else {
            return;
        };

        let result: Result<(), String> = with_interp(|interp| interp.enter(|vm| {
            let dict = globals.downcast::<pvm::builtins::PyDict>().map_err(|_| "globals не dict".to_string())?;
            let func = dict.get_item("on_event", vm).map_err(|e| format_py_exception(vm, &e))?;

            let args = FuncArgs::from(vec![
                vm.ctx.new_int(event_type).into(),
                vm.ctx.new_float(data[0] as f64).into(),
                vm.ctx.new_float(data[1] as f64).into(),
                vm.ctx.new_float(data[2] as f64).into(),
                vm.ctx.new_float(data[3] as f64).into(),
            ]);
            func.call(args, vm).map_err(|e| format_py_exception(vm, &e))?;
            Ok(())
        }));

        if let Err(e) = result {
            eprintln!("[python-script] ошибка в on_event() ('{}'): {}", self.source_path, e);
            self.load_error = Some(e);
        }
    }
}

/// Небольшой хелпер: `PyBaseException` сам по себе печатается неинформативно
/// (`PyBaseException`, без текста) — rustpython-vm умеет достать
/// человеко-читаемое сообщение только через `vm.enter()`-контекст, что и
/// делает эта функция единообразно во всех местах выше.
fn format_py_exception(vm: &pvm::VirtualMachine, exc: &pvm::builtins::PyBaseExceptionRef) -> String {
    let mut s = String::new();
    if vm.write_exception(&mut s, exc).is_err() {
        return "не удалось отформатировать исключение Python".to_string();
    }
    s
}
