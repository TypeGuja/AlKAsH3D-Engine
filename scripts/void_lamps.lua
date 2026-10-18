-- void_lamps.lua — прожекторы на штативах в пустоте (restroom, после падения).
-- Исполняется рантаймом alkash3d-luascript (DLL); у каждой лампы своё
-- Lua-состояние из этого файла, поэтому переменные ниже — свои у каждой.
-- Файл перечитывается при сбросе на R.
--
-- update(dt, x, y, z, index, t, _) -> (яркость, 0, 0, 1)
--   x, y, z — где стоит лампа; index — её номер: 1..4 — над столом,
--   5..6 — перед пробковой доской; t — секунды с появления в пустоте.
--   Яркость: 0 — не горит, 1 — норма.
-- on_event(kind, a, b, c, d) — события игры, kind 0 = Custom:
--   a == 1 — всплеск, лампа сбивается в мерцание (b — сколько секунд);
--   a == 2 — бланк отказа подписан: загораются лампы доски.

local FIRST_BOARD_LAMP = 5

local me           -- номер этой лампы
local start        -- когда лампа начинает загораться (nil — ещё не пора)
local next_flick   -- когда следующее случайное мерцание
local flick = 0    -- сколько ещё мерцать
local hold = 0     -- сколько держать текущий уровень мерцания
local level = 1
local now = 0

local function smooth(x)
    x = math.min(math.max(x, 0), 1)
    return x * x * (3 - 2 * x)
end

function update(dt, x, y, z, index, t, _)
    now = t
    if me == nil then
        me = math.floor(index)
        math.randomseed(me * 7919)
        if me < FIRST_BOARD_LAMP then
            -- Над столом загораются по очереди, у каждой — своя задержка.
            start = 0.8 + (me - 1) * 1.1 + math.random() * 0.4
            next_flick = start + 8 + math.random() * 10
        end
    end
    if start == nil then
        return 0, 0, 0, 1
    end

    -- Плавное появление: стол за 3 с, доска медленнее — за 5 с.
    local fade = (me < FIRST_BOARD_LAMP) and 3.0 or 5.0
    local on = smooth((t - start) / fade)

    -- Иногда лампа ненадолго начинает мерцать.
    if t > next_flick then
        flick = 0.3 + math.random() * 0.7
        next_flick = t + 6 + math.random() * 16
    end
    if flick > 0 then
        flick = flick - dt
        hold = hold - dt
        if hold <= 0 then
            -- Новый уровень каждые 40–120 мс, а не каждый кадр.
            hold = 0.04 + math.random() * 0.08
            level = (math.random() < 0.45) and (0.05 + math.random() * 0.25) or (0.8 + math.random() * 0.3)
        end
    else
        level = 1
    end

    return on * level, 0, 0, 1
end

function on_event(kind, a, b, c, d)
    if kind ~= 0 then
        return
    end
    if a == 1 then
        flick = (b > 0) and b or 0.5
    elseif a == 2 and me ~= nil and me >= FIRST_BOARD_LAMP and start == nil then
        -- Сначала пауза (машинка убирается), лампы доски — с разницей в секунду.
        start = now + 1.0 + (me - FIRST_BOARD_LAMP) * 1.0
        next_flick = start + 12 + math.random() * 10
    end
end
