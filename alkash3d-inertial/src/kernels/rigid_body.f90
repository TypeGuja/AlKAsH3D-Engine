! inertial/src/kernels/rigid_body.f90
module rigid_body_mod
    use, intrinsic :: iso_c_binding
    implicit none

    type, bind(c) :: rigid_body_c
        real(c_float) :: position(3)
        real(c_float) :: velocity(3)
        real(c_float) :: acceleration(3)
        real(c_float) :: angular_velocity(3)
        real(c_float) :: angular_acceleration(3)
        real(c_float) :: inertia(3, 3)
        real(c_float) :: inv_inertia(3, 3)
        real(c_float) :: mass
        real(c_float) :: inv_mass
        real(c_float) :: restitution
        real(c_float) :: friction
        real(c_float) :: linear_damping
        real(c_float) :: angular_damping
        integer(c_int) :: is_static
        integer(c_int) :: is_asleep
        ! ДОБАВЛЕНО (физика автомобиля — вращение кузова): раньше эта
        ! модель физики вообще не отслеживала ориентацию тела в мире (см.
        ! исторический комментарий в solver.f90/solve_ball_joint — там это
        ! явно оговорено как ограничение). angular_velocity/angular_
        ! acceleration уже существовали и интегрировались в
        ! integrate_bodies/batch_integrate, но результат — угловая
        ! СКОРОСТЬ — никуда не накапливался в реальный поворот тела.
        ! orientation — кватернион (x, y, z, w), единичный при
        ! is_static=1 или в состоянии покоя. ВАЖНО: добавлено В КОНЕЦ
        ! структуры — не переставляет и не меняет существующие поля,
        ! поэтому не ломает layout уже написанного (не-автомобильного)
        ! кода, который заполняет rigid_body_c по имени поля, а не по
        ! смещению.
        real(c_float) :: orientation(4)
        ! ДОБАВЛЕНО (код-ревью — per-body радиус вместо одного
        ! глобального BODY_RADIUS=0.5 на все тела без исключения, см.
        ! narrow_phase.f90): опять же добавлено В КОНЕЦ структуры, та же
        ! причина, что у orientation выше.
        real(c_float) :: radius
        ! ДОБАВЛЕНО (реальная физика машины — box-коллайдер кузова):
        ! `shape_type` — 0 = сфера (использует `radius` выше, ЕДИНСТВЕННАЯ
        ! форма, которую честно понимает Fortran-узкая фаза
        ! `narrow_phase_gjk` в narrow_phase.f90), 1 = коробка (половинные
        ! размеры в `half_extents`, локальные оси, повёрнутые `orientation`).
        ! Fortran-солвер (narrow_phase.f90/solve_contacts_vectorized) по-
        ! прежнему знает ТОЛЬКО сферы — box-vs-sphere/box-vs-plane узкая
        ! фаза добавлена на Rust-стороне (`alkash3d-inertial/src/lib.rs`,
        ! рядом с уже существующим `resolve_plane_contacts`, тем же
        ! приёмом), которая для box-тел готовит normal/penetration/point
        ! САМА и передаёт их в тот же самый (неизменный) Fortran-солвер
        ! контактов — так весь риск ABI-изменения ограничен ДОБАВЛЕНИЕМ
        ! полей в конец структуры, без единой правки уже отлаженного
        ! Fortran-кода узкой фазы/солвера. `shape_type`/`half_extents`
        ! добавлены и сюда (bind(c)-структуру), а не только в Rust-ABI,
        ! потому что этот массив `bodies` — общая память между Rust и
        ! Fortran (integrate_bodies/solve_contacts_vectorized индексируют
        ! его напрямую) — layout обязан совпадать побайтово на обеих
        ! сторонах, даже если сам Fortran-код эти два поля не читает.
        integer(c_int) :: shape_type
        real(c_float) :: half_extents(3)
    end type rigid_body_c

    type, bind(c) :: contact_c
        integer(c_int) :: body_a
        integer(c_int) :: body_b
        real(c_float) :: normal(3)
        real(c_float) :: penetration
        real(c_float) :: point(3)
        real(c_float) :: tangent1(3)
        real(c_float) :: tangent2(3)
        real(c_float) :: friction_impulse(2)
    end type contact_c

    ! ДОБАВЛЕНО (разборка машины на детали — джойнты/constraint API,
    ! см. подробное обоснование в solver.f90): типы соединений,
    ! доступные `constraint_c%joint_type`. Плоские integer-константы, а не
    ! Fortran enum — bind(c) не умеет экспортировать именованные
    ! константы как C-символы без отдельной обёртки, поэтому оба конца
    ! ABI (эта константа и Rust-сторона в `alkash3d-inertial/src/lib.rs`
    ! + `alkash3d-rust/src/plugin/physics_api.rs`) держат СОВПАДАЮЩИЕ
    ! литералы вручную — тот же принцип, что уже применяется здесь для
    ! layout `rigid_body_c`/`constraint_c` целиком (совпадение по
    ! соглашению, а не по общему заголовку).
    integer(c_int), parameter :: JOINT_BALL = 0   ! шаровой шарнир — только точка крепления, вращение свободно по всем осям
    integer(c_int), parameter :: JOINT_HINGE = 1  ! петля — точка крепления + вращение только вокруг axis_a (дверь, капот)
    integer(c_int), parameter :: JOINT_FIXED = 2  ! жёсткая сварка/болтовое соединение — точка крепления + вращение полностью заперто
    integer(c_int), parameter :: JOINT_SLIDER = 3 ! ползун — свободное смещение вдоль axis_a, перпендикулярные оси заперты

    type, bind(c) :: constraint_c
        integer(c_int) :: body_a
        integer(c_int) :: body_b
        ! ДОБАВЛЕНО: см. JOINT_* выше. Значение по умолчанию (0 при
        ! нулевой инициализации структуры на Rust-стороне) — JOINT_BALL,
        ! то есть старое поведение (единственный тип соединения, который
        ! существовал до этой доработки) остаётся поведением по
        ! умолчанию для кода, который заполняет constraint_c частично.
        integer(c_int) :: joint_type
        real(c_float) :: anchor_a(3)
        real(c_float) :: anchor_b(3)
        ! ДОБАВЛЕНО (JOINT_HINGE/JOINT_SLIDER): направление оси в
        ! мировых координатах. Как и anchor_a/anchor_b, ось НЕ
        ! поворачивается вместе с телом (та же осознанная упрощённая
        ! модель, что уже описана у anchor_a/anchor_b в solve_ball_joint
        ! ниже — честный учёт вращения потребовал бы хранить точку/ось
        ! в локальной системе координат тела и разворачивать её через
        ! orientation каждый кадр, это отдельная более крупная доработка).
        ! axis_b хранится отдельно (не переиспользует axis_a) для
        ! симметрии с anchor_a/anchor_b и на будущее, когда per-body
        ! разворот оси всё же появится — солвер ниже сейчас читает
        ! ТОЛЬКО axis_a, вызывающая сторона обязана передавать в axis_b
        ! то же самое мировое направление.
        real(c_float) :: axis_a(3)
        real(c_float) :: axis_b(3)
        real(c_float) :: bias
        ! ДОБАВЛЕНО: пороги разрушения — независимые для линейной
        ! (растяжение/сдвиг в точке крепления) и угловой (скручивание/
        ! изгиб) нагрузки, т.к. у реального болтового/сварного соединения
        ! это физически разные механизмы отказа с разными порогами.
        ! <= 0 означает "неразрушимо" — сознательно выбран БЕЗОПАСНЫЙ
        ! дефолт: структура, заполненная нулями (как любой Default на
        ! Rust-стороне), не должна внезапно рассыпаться в первом же
        ! кадре просто потому, что порог не был явно задан.
        real(c_float) :: break_impulse_linear
        real(c_float) :: break_impulse_angular
        ! ИЗМЕНЕНО: было `accumulated_impulse` — единственный скаляр,
        ! используемый для warm-starting, который старый solve_ball_joint
        ! честно предупреждал "если джойны когда-нибудь понадобятся в
        ! игре, здесь надо расширить до вектора" (см. историю в
        ! solver.f90). Сейчас именно этот момент настал: `linear_impulse`/
        ! `angular_impulse` — векторы, обнуляемые в начале КАЖДОГО
        ! физического шага (см. solve_constraints в solver.f90) и
        ! накапливающие суммарную impulse-коррекцию ЭТОГО шага — то есть
        ! ровно ту величину, с которой имеет смысл сравнивать
        ! break_impulse_linear/angular, а не warm-start кэш (см. solver.f90
        ! за подробным обоснованием отказа от накопления между кадрами).
        real(c_float) :: linear_impulse(3)
        real(c_float) :: angular_impulse(3)
        ! ДОБАВЛЕНО: выходной флаг — 1, как только суммарный импульс ЛЮБОГО
        ! из порогов (линейного/углового) этого шага превысил свой
        ! break_impulse_*. Солвер сам пропускает (`cycle`) уже сломанные
        ! соединения — Rust-сторона решает, удалять ли запись констрейнта
        ! полностью (`remove_constraint`) или оставить её для отладки/
        ! разового уведомления игрового кода о поломке.
        integer(c_int) :: is_broken
    end type constraint_c

contains
    ! ===================================================================
    ! ОБЩИЕ КВАТЕРНИОННЫЕ ХЕЛПЕРЫ
    ! ===================================================================
    ! ДОБАВЛЕНО (raycast против box-тел + box-vs-box narrow phase, см.
    ! raycast.f90/narrow_phase.f90): вынесены сюда (а не продублированы в
    ! каждом модуле по отдельности), потому что ОБА этих модуля уже
    ! используют `rigid_body_mod` (за `rigid_body_c`) — общая зависимость,
    ! а не дополнительная. Поворот вектора `v` единичным кватернионом `q`
    ! (x,y,z,w) — эффективная формула без построения полной матрицы
    ! поворота: v' = v + 2*w*(qv×v) + 2*(qv×(qv×v)), где qv — векторная
    ! часть кватерниона.
    pure function rotate_vec_by_quat(v, q) result(vr)
        real(c_float), intent(in) :: v(3), q(4)
        real(c_float) :: vr(3), qv(3), t(3)
        qv = q(1:3)
        t(1) = 2.0 * (qv(2)*v(3) - qv(3)*v(2))
        t(2) = 2.0 * (qv(3)*v(1) - qv(1)*v(3))
        t(3) = 2.0 * (qv(1)*v(2) - qv(2)*v(1))
        vr(1) = v(1) + q(4)*t(1) + (qv(2)*t(3) - qv(3)*t(2))
        vr(2) = v(2) + q(4)*t(2) + (qv(3)*t(1) - qv(1)*t(3))
        vr(3) = v(3) + q(4)*t(3) + (qv(1)*t(2) - qv(2)*t(1))
    end function rotate_vec_by_quat

    ! Сопряжённый (= обратный для ЕДИНИЧНОГО) кватернион — переводит вектор
    ! из мировых координат в локальные оси тела.
    pure function quat_conjugate(q) result(qc)
        real(c_float), intent(in) :: q(4)
        real(c_float) :: qc(4)
        qc = [-q(1), -q(2), -q(3), q(4)]
    end function quat_conjugate

    ! ===================================================================
    ! ИНТЕГРИРОВАНИЕ КВАТЕРНИОНА ОРИЕНТАЦИИ ПО УГЛОВОЙ СКОРОСТИ
    ! ===================================================================
    ! ДОБАВЛЕНО (физика автомобиля — вращение кузова): стандартная формула
    ! интеграции ориентации — q(t+dt) = normalize(q(t) + 0.5*dt*(0,w)*q(t)),
    ! где (0,w) — "чистый" кватернион угловой скорости (0, wx, wy, wz), а
    ! умножение — обычное кватернионное произведение. Используется как
    ! приближение первого порядка (Эйлер), достаточное при малых dt (~1/60
    ! с) и не требующее хранить полную матрицу вращения — тот же уровень
    ! точности, что уже применяется для линейной части (integrate_bodies
    ! ниже тоже простой Эйлер, не RK4). Нормализация после каждого шага
    ! обязательна — без неё q постепенно "уезжает" от единичной длины
    ! из-за накопления ошибки округления, что на глаз проявляется как
    ! постепенное "разбухание"/искажение вращения тела за много кадров.
    subroutine integrate_orientation(q, angular_velocity, dt)
        implicit none
        real(c_float), intent(inout) :: q(4)
        real(c_float), intent(in) :: angular_velocity(3)
        real(c_float), intent(in) :: dt
        real(c_float) :: dq(4), q_new(4), qlen
        ! Кватернионное произведение (0, wx, wy, wz) * q, q=(x,y,z,w):
        ! стандартная формула произведения Гамильтона.
        dq(1) = angular_velocity(2)*q(3) - angular_velocity(3)*q(2) + angular_velocity(1)*q(4)
        dq(2) = angular_velocity(3)*q(1) - angular_velocity(1)*q(3) + angular_velocity(2)*q(4)
        dq(3) = angular_velocity(1)*q(2) - angular_velocity(2)*q(1) + angular_velocity(3)*q(4)
        dq(4) = -angular_velocity(1)*q(1) - angular_velocity(2)*q(2) - angular_velocity(3)*q(3)

        q_new = q + dq * (0.5 * dt)
        qlen = sqrt(q_new(1)**2 + q_new(2)**2 + q_new(3)**2 + q_new(4)**2)
        if (qlen > 1.0e-8) then
            q = q_new / qlen
        end if
    end subroutine integrate_orientation

    ! ===================================================================
    ! ИНТЕГРИРОВАНИЕ
    ! ===================================================================
    subroutine integrate_bodies(bodies, n, dt) bind(c, name="integrate_bodies")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(inout) :: bodies(n)
        integer(c_int), intent(in), value :: n
        real(c_float), intent(in), value :: dt
        integer :: i

        do i = 1, n
            if (bodies(i)%is_asleep == 0 .and. bodies(i)%is_static == 0) then
                bodies(i)%velocity = bodies(i)%velocity + bodies(i)%acceleration * dt
                bodies(i)%velocity = bodies(i)%velocity * (1.0 - bodies(i)%linear_damping * dt)
                bodies(i)%position = bodies(i)%position + bodies(i)%velocity * dt

                bodies(i)%angular_velocity = bodies(i)%angular_velocity + &
                        bodies(i)%angular_acceleration * dt
                bodies(i)%angular_velocity = bodies(i)%angular_velocity * &
                        (1.0 - bodies(i)%angular_damping * dt)

                call integrate_orientation(bodies(i)%orientation, bodies(i)%angular_velocity, dt)
            end if
        end do
    end subroutine integrate_bodies

    ! ===================================================================
    ! SOLVER КОНТАКТОВ
    ! ===================================================================
    subroutine solve_contacts(bodies, contacts, n_contacts, iterations) &
            bind(c, name="solve_contacts")
        use, intrinsic :: iso_c_binding
        implicit none
        ! ИСПРАВЛЕНО: assumed-shape (`bodies(:)`/`contacts(:)`) не гарантированно
        ! C-совместимы для bind(c)-процедур (компилятор может ожидать дескриптор
        ! массива вместо простого указателя, который передаёт Rust) —
        ! assumed-size (`bodies(*)`) и explicit-shape (`contacts(n_contacts)`)
        ! однозначно соответствуют "сырому указателю", как их и передаёт Rust.
        type(rigid_body_c), intent(inout) :: bodies(*)
        type(contact_c), intent(inout) :: contacts(n_contacts)
        integer(c_int), intent(in), value :: n_contacts, iterations
        integer :: iter, i

        do iter = 1, iterations
            do i = 1, n_contacts
                call resolve_contact_simple(bodies(contacts(i)%body_a+1), &
                        bodies(contacts(i)%body_b+1), &
                        contacts(i)%normal, &
                        contacts(i)%penetration)
            end do
        end do
    end subroutine solve_contacts

    subroutine resolve_contact_simple(a, b, normal, penetration)
        implicit none
        type(rigid_body_c), intent(inout) :: a, b
        real(c_float), intent(in) :: normal(3)
        real(c_float), intent(in) :: penetration
        real(c_float) :: rel_vel(3), vel_along, impulse
        real(c_float) :: restitution, inv_mass_sum, impulse_vec(3), correction(3)
        ! ИСПРАВЛЕНО (найдено по жалобе пользователя: тела проваливались
        ! сквозь статичный "пол" из сфер, хотя narrow_phase честно находил
        ! контакт с правильной глубиной проникновения): позиционная
        ! коррекция ниже раньше ВСЕГДА делилась 50/50 между `a` и `b`, не
        ! глядя на `is_static` — то есть КАЖДЫЙ контакт с неподвижной
        ! опорой (a, is_static=1) на самом деле сдвигал саму опору на
        ! половину глубины проникновения, а падающему телу (b) доставалась
        ! только вторая половина. Хуже того, `solve_contacts` вызывается
        ! `solver_iterations` (8) раз ЗА КАДР — "статичная" опора реально
        ! смещалась 8 раз за кадр от каждого контакта, у которого она
        ! участница, и эти смещения накапливались кадр к кадру (позиция
        ! `a` никогда не восстанавливается назад). Скоростная коррекция
        ! выше (через inv_mass_sum) уже была правильной — при inv_mass=0 у
        ! статичного тела весь импульс скорости и так уходит в `b`, баг
        ! был именно в этом отдельном блоке позиционной коррекции.
        !
        ! Фикс — распределяем коррекцию пропорционально inv_mass (как и
        ! скоростной импульс выше): тело с inv_mass=0 (статичное) получает
        ! РОВНО нулевую долю коррекции, вся коррекция глубины уходит
        ! динамическому телу. Если оба тела динамические — соотношение
        ! долей то же самое (пропорционально их inv_mass), что физически
        ! корректнее старого фиксированного 50/50 (тяжёлое тело должно
        ! сдвигаться меньше лёгкого).
        real(c_float) :: total_inv_mass, share_a, share_b

        rel_vel = b%velocity - a%velocity
        vel_along = rel_vel(1)*normal(1) + rel_vel(2)*normal(2) + rel_vel(3)*normal(3)

        if (vel_along < 0.0) then
            restitution = (a%restitution + b%restitution) * 0.5
            impulse = -(1.0 + restitution) * vel_along
            inv_mass_sum = a%inv_mass + b%inv_mass

            if (inv_mass_sum > 0.0) then
                impulse = impulse / inv_mass_sum
                impulse_vec = normal * impulse
                a%velocity = a%velocity - impulse_vec * a%inv_mass
                b%velocity = b%velocity + impulse_vec * b%inv_mass
            end if
        end if

        total_inv_mass = a%inv_mass + b%inv_mass
        if (total_inv_mass > 0.0) then
            share_a = a%inv_mass / total_inv_mass
            share_b = b%inv_mass / total_inv_mass
        else
            ! Оба тела статичны (не должно происходить при нормальной
            ! настройке сцены — статичные тела с статичными не должны
            ! рождать контакты, которые кто-то пытается решить), но на
            ! всякий случай не двигаем ни одно из них вместо деления на 0.
            share_a = 0.0
            share_b = 0.0
        end if

        correction = normal * penetration
        a%position = a%position - correction * share_a
        b%position = b%position + correction * share_b
    end subroutine resolve_contact_simple

    ! ===================================================================
    ! ОБНОВЛЕНИЕ AABB
    ! ===================================================================
    subroutine update_aabb(bodies, n, min_bounds, max_bounds, radius) &
            bind(c, name="update_aabb")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(in) :: bodies(n)
        integer(c_int), intent(in), value :: n
        real(c_float), intent(out) :: min_bounds(n, 3)
        real(c_float), intent(out) :: max_bounds(n, 3)
        real(c_float), intent(in), value :: radius
        integer :: i

        do i = 1, n
            min_bounds(i, 1) = bodies(i)%position(1) - radius
            min_bounds(i, 2) = bodies(i)%position(2) - radius
            min_bounds(i, 3) = bodies(i)%position(3) - radius
            max_bounds(i, 1) = bodies(i)%position(1) + radius
            max_bounds(i, 2) = bodies(i)%position(2) + radius
            max_bounds(i, 3) = bodies(i)%position(3) + radius
        end do
    end subroutine update_aabb
end module rigid_body_mod
