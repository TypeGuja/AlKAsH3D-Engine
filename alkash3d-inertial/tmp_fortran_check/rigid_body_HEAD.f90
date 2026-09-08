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

    type, bind(c) :: constraint_c
        integer(c_int) :: body_a
        integer(c_int) :: body_b
        real(c_float) :: anchor_a(3)
        real(c_float) :: anchor_b(3)
        real(c_float) :: bias
        real(c_float) :: accumulated_impulse
    end type constraint_c

contains
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
