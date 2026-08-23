# inertial — физический плагин Alkash3D (исправленная версия)

## Что изменилось по сравнению с оригиналом

### Fortran (`src/kernels/`)
- **`broad_phase.f90`** — **критичный фикс** (это и вызывало
  `STATUS_ACCESS_VIOLATION` при запуске): `broad_phase_grid` читал и писал
  ОДИН И ТОТ ЖЕ массив `cell_pairs` одновременно (компактный список тел по
  ячейкам и результирующие пары жили в одной памяти), плюс запись велась
  без проверки границ буфера. Теперь это два разных массива (`body_list`
  — локальный scratch, `cell_pairs` — только вывод), и добавлен параметр
  `max_pairs`, жёстко ограничивающий запись пределами переданного буфера.
  Rust-обёртка (`find_pairs_grid`) теперь проверяет, не было ли реальных
  пар больше буфера, и при необходимости увеличивает буфер и перевызывает.
- **`kernels_optimized.f90`** — устранены гонки данных в `generate_collision_pairs`
  и `broad_phase_temporal` (reduction-переменная использовалась как индекс
  записи в общий массив — теперь atomic capture). `solve_contacts_vectorized`
  и `resolve_penetration_batch` были небезопасны для параллельного
  исполнения (гонка на общих телах между контактами) — теперь используют
  `atomic update` покомпонентно. `update_sleep_state` получил реальный
  таймер-гистерезис (`sleep_timers`) — раньше тела никогда не засыпали.
- **`narrow_phase.f90`** — фиктивный "GJK для любых форм" (хардкод на сферы
  0.5 + заглушки penetration=0.5/ненормированная нормаль) заменён честным
  корректным sphere-sphere тестом.
- **`solver.f90`** — `solve_ball_joint` считал "ошибку" сустава так, что она
  алгебраически всегда была константой (не зависела от движения тел), и
  корректировал только X-ось. Теперь ошибка — полный 3D-вектор, зависящий
  от текущих позиций тел.
- **`rigid_body.f90`** — без изменений, уже содержал корректные
  `integrate_bodies`/`solve_contacts`/`update_aabb` (раньше я по ошибке
  решил, что их не хватает, и завёл отдельный `reference.f90` с теми же
  именами — это создавало бы риск конфликта символов при линковке;
  `reference.f90` убран, эти три функции уже есть здесь).

### Rust (`src/ffi/mod.rs`, `src/lib.rs`, `build.rs`)
- **`ffi/mod.rs`** — исправлена несовпадающая сигнатура `broad_phase_sap_optimized`
  (отсутствовал параметр `n`), добавлен параметр `sleep_timers`,
  `batch_integrate` теперь ПО-НАСТОЯЩЕМУ многопоточный (`std::thread::scope`
  + `split_at_mut` на непересекающиеся срезы), `find_pairs_grid` теперь
  передаёт `max_pairs` и умеет расти при переполнении (см. выше).
- **`lib.rs`** (был самодельной O(N²) реализацией без Fortran) — теперь
  реальный мост между ABI движка (`PhysicsBody` и т.п.) и Fortran-солвером.
  Заодно исправлены: баг в `resolve_contact` (коррекция проникновения брала
  только `normal[0]` и применяла его ко всем трём осям), `remove_body`
  сдвигал ID всех тел после удалённого (теперь стабильные handle),
  `println!` на каждый кадр физики.
- **`build.rs`** (новый) — компилирует `.f90` через `gfortran` и линкует
  статическую библиотеку в `inertial.dll`.
- **`.cargo/config.toml`** (новый) — фиксирует GNU-таргет
  (`x86_64-pc-windows-gnu`) по умолчанию на Windows, чтобы линковка
  Fortran-рантайма (MinGW) не конфликтовала с MSVC-линкером.

## Сборка

Нужен установленный **gfortran**:
- Linux: `sudo apt install gfortran`
- macOS: `brew install gcc` (даёт gfortran)
- Windows: MSYS2 (https://www.msys2.org/) → открыть **MSYS2 MinGW64** →
  ```
  pacman -Syu
  pacman -S mingw-w64-x86_64-gcc-fortran mingw-w64-x86_64-toolchain
  ```
  Добавить `C:\msys64\mingw64\bin` в PATH (и перезапустить терминал/IDE).
  Установить GNU Rust-таргет:
  ```
  rustup toolchain install stable-x86_64-pc-windows-gnu
  rustup target add x86_64-pc-windows-gnu
  ```
  `.cargo/config.toml` в этом крейте уже фиксирует таргет — просто:
  ```
  cargo clean
  cargo build --release
  ```
  (движок на MSVC трогать не нужно — DLL грузится в рантайме через
  `LoadLibrary`, а не линкуется статически, так что GNU-DLL и MSVC-.exe
  прекрасно работают вместе через `extern "C"`).

На Linux/macOS достаточно обычного `cargo build --release`.

## Тест производительности

```
cargo run --release --example perf_test
```
Прогоняет реальный ABI (`get_plugin_api` → `init` → `add_body` → `update`
→ `get_stats`) на трёх сценариях (разрежённая сцена, плотная сетка,
падающая куча) и разном числе тел, печатает разбивку по broad/narrow/
solver-времени и условный FPS.

## Что НЕ трогал

- **`body.rs`/`world.rs`/`broad_phase.rs`(rust)/`collision.rs`/`solver.rs`(rust)/
  `math.rs`/`simd_math.rs`** (альтернативная реализация `PhysicsWorld` на
  `nalgebra`+`rayon`) — раз выбор был в пользу Fortran, это осталось
  неиспользуемым кодом, как и было. Не редактировал.
- **Constraints/joints** — `solve_constraints` теперь физически корректен,
  но в ABI движка (`PhysicsAPI`) до сих пор нет функции для СОЗДАНИЯ
  constraint'ов (`add_constraint`) — то есть игра пока не может создать
  сустав, даже если солвер их умеет считать. Это отдельная доработка ABI.
