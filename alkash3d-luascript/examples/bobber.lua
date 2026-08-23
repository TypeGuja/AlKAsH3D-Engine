-- bobber.lua — эталонный пример .lua-скрипта для alkash3d-luascript.
--
-- Соглашение по протоколу (см. комментарий в src/lib.rs):
--   update(dt, x, y, z, rx, ry, rz) -> (new_x, new_y, new_z, changed)
--     вызывается каждый кадр; changed=1 (число), если позицию нужно
--     применить, иначе 0 — движок оставит Transform как есть.
--   on_event(event_type, data0, data1, data2, data3)
--     вызывается по событию (см. ScriptEventType в scripting_api.rs:
--     0=Custom, 1=CollisionEnter, 2=CollisionExit, 3=ZoneEnter,
--     4=ZoneExit, 5=Spawned, 6=Despawned).
--
-- Демонстрирует ровно то же самое, что и Rust-пример
-- (alkash3d-examplescript) — покадровое покачивание вверх-вниз по
-- синусоиде + изменение амплитуды по событию ZoneEnter/Custom, чтобы
-- было наглядно видно: один и тот же сценарий работает одинаково и на
-- нативном (C++/Rust), и на Lua языке скриптинга.

local base_x, base_y, base_z = nil, nil, nil
local time_accum = 0.0
local amplitude = 0.3    -- метры
local bob_speed = 2.0    -- рад/сек

function update(dt, x, y, z, rx, ry, rz)
    -- Точка покоя фиксируется один раз, при первом кадре — дальше
    -- покачивание идёт вокруг НЕЁ, а не вокруг текущей позиции каждого
    -- кадра (иначе амплитуда накапливалась бы, а не колебалась).
    if base_x == nil then
        base_x, base_y, base_z = x, y, z
    end

    time_accum = time_accum + dt
    local offset_y = math.sin(time_accum * bob_speed) * amplitude

    return base_x, base_y + offset_y, base_z, 1
end

function on_event(event_type, data0, data1, data2, data3)
    -- ZoneEnter (3) или Custom (0): data0 > 0 — новый множитель амплитуды
    -- относительно базовой (0.3) — то же соглашение, что и в
    -- Rust-примере, чтобы оба скрипта были взаимозаменяемы для теста.
    if event_type == 3 or event_type == 0 then
        if data0 > 0.0 then
            amplitude = 0.3 * data0
        end
    end
end
