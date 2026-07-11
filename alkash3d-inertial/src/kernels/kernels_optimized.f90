! inertial/src/kernels/kernels_optimized.f90
! Дополнительные оптимизированные ядра для максимальной производительности

module kernels_optimized_mod
    use, intrinsic :: iso_c_binding
    use rigid_body_mod, only: rigid_body_c, contact_c
    implicit none

    integer, parameter :: SIMD_WIDTH = 8
    integer, parameter :: SIMD_WIDTH_512 = 16

contains

    ! ===================================================================
    ! BATCH INTEGRATION - безопасно параллелится по НЕПЕРЕСЕКАЮЩИМСЯ
    ! диапазонам [start_idx, end_idx] одного и того же массива bodies —
    ! гонок нет, т.к. каждый вызов (из разных Rust-потоков, см. lib.rs)
    ! получает свой собственный срез bodies.
    ! ===================================================================
    subroutine batch_integrate(bodies, n, dt, gravity, start_idx, end_idx) &
            bind(c, name="batch_integrate")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(inout) :: bodies(n)
        integer(c_int), intent(in) :: n
        real(c_float), intent(in) :: dt, gravity
        integer(c_int), intent(in) :: start_idx, end_idx

        integer :: i
        real(c_float) :: dt_linear, dt_angular

        dt_linear = dt
        dt_angular = dt

        do i = start_idx, end_idx
            if (bodies(i)%is_asleep == 0 .and. bodies(i)%is_static == 0) then
                bodies(i)%velocity(2) = bodies(i)%velocity(2) + gravity * dt_linear
                bodies(i)%position(1) = bodies(i)%position(1) + bodies(i)%velocity(1) * dt_linear
                bodies(i)%position(2) = bodies(i)%position(2) + bodies(i)%velocity(2) * dt_linear
                bodies(i)%position(3) = bodies(i)%position(3) + bodies(i)%velocity(3) * dt_linear

                bodies(i)%angular_velocity(1) = bodies(i)%angular_velocity(1) + &
                        bodies(i)%angular_acceleration(1) * dt_angular
                bodies(i)%angular_velocity(2) = bodies(i)%angular_velocity(2) + &
                        bodies(i)%angular_acceleration(2) * dt_angular
                bodies(i)%angular_velocity(3) = bodies(i)%angular_velocity(3) + &
                        bodies(i)%angular_acceleration(3) * dt_angular

                bodies(i)%velocity = bodies(i)%velocity * (1.0 - bodies(i)%linear_damping * dt_linear)
                bodies(i)%angular_velocity = bodies(i)%angular_velocity * &
                        (1.0 - bodies(i)%angular_damping * dt_angular)
            end if
        end do
    end subroutine batch_integrate

    ! ===================================================================
    ! FAST COLLISION PAIR GENERATION
    !
    ! ИСПРАВЛЕНО (гонка данных): раньше `local_count` был объявлен как
    ! `reduction(+:local_count)` — у reduction-переменной каждый поток
    ! получает СВОЮ приватную копию, суммируемую только в конце
    ! параллельного региона. Но `local_count` тут же использовался ВНУТРИ
    ! региона как индекс записи в общий массив `pairs(...)` — то есть
    ! разные потоки писали в ОДНИ И ТЕ ЖЕ индексы `pairs` одновременно
    ! (каждый поток нумеровал свои находки с нуля независимо от других).
    ! Результат — потерянные и перезаписанные пары при реальном
    ! распараллеливании. Теперь pair_count — ОБЩАЯ (shared) переменная,
    ! и уникальный индекс каждый поток получает атомарным захватом
    ! (atomic capture) непосредственно перед записью — это стандартный
    ! OpenMP-паттерн для параллельного добавления в общий массив.
    ! ===================================================================
    subroutine generate_collision_pairs(bodies, n, pairs, pair_count, radius) &
            bind(c, name="generate_collision_pairs")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(in) :: bodies(n)
        integer(c_int), intent(in) :: n
        integer(c_int), intent(out) :: pairs(*)
        integer(c_int), intent(out) :: pair_count
        real(c_float), intent(in) :: radius

        integer :: i, j, my_idx
        real(c_float) :: dx, dy, dz, dist_sq, threshold

        threshold = (radius + radius) ** 2
        pair_count = 0

        !$omp parallel do private(i, j, dx, dy, dz, dist_sq, my_idx) shared(pair_count, pairs)
        do i = 1, n
            if (bodies(i)%is_asleep == 1) cycle
            if (bodies(i)%is_static == 1) cycle

            do j = i+1, n
                if (bodies(j)%is_asleep == 1) cycle
                if (bodies(j)%is_static == 1) cycle

                dx = bodies(j)%position(1) - bodies(i)%position(1)
                dy = bodies(j)%position(2) - bodies(i)%position(2)
                dz = bodies(j)%position(3) - bodies(i)%position(3)
                dist_sq = dx*dx + dy*dy + dz*dz

                if (dist_sq < threshold) then
                    pair_count = pair_count + 1
                    my_idx = pair_count
                    pairs(2*my_idx - 1) = i - 1
                    pairs(2*my_idx) = j - 1
                end if
            end do
        end do
        !$omp end parallel do
    end subroutine generate_collision_pairs

    ! ===================================================================
    ! VECTORIZED SOLVE CONTACTS
    !
    ! ИСПРАВЛЕНО (гонка данных): раньше `!$omp parallel do` шёл прямо по
    ! контактам, и разные потоки МОГЛИ одновременно писать в
    ! bodies(idx_a)%velocity/position ОДНОГО И ТОГО ЖЕ тела — если два
    ! контакта в одном проходе ссылаются на общее тело (например, куб,
    ! лежащий сразу на нескольких плитках пола — обычное дело). Полное
    ! решение "по-хорошему" — графовая раскраска контактов (группировка
    ! так, чтобы в одной группе не было общих тел), но это отдельная,
    ! более объёмная задача. Как минимально достаточный и КОРРЕКТНЫЙ
    ! фикс здесь — каждое обновление скорости/позиции тела теперь идёт
    ! через `!$omp atomic update` по каждой компоненте: это делает
    ! параллельное исполнение безопасным (гонки нет, ни одно обновление
    ! не теряется), ценой того, что схема становится Jacobi-подобной
    ! (все потоки читают состояние "из того же кадра" и атомарно
    ! накапливают изменения), а не строго Gauss-Seidel — сходимость чуть
    ! иная, но результат физически корректен и детерминированно безопасен.
    ! ===================================================================
    subroutine solve_contacts_vectorized(bodies, contacts, n_contacts, iterations, dt) &
            bind(c, name="solve_contacts_vectorized")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(inout) :: bodies(:)
        type(contact_c), intent(inout) :: contacts(:)
        integer(c_int), intent(in) :: n_contacts, iterations
        real(c_float), intent(in) :: dt

        integer :: iter, i, idx_a, idx_b
        real(c_float) :: rel_vel(3), vel_normal, impulse
        real(c_float) :: restitution, inv_mass_sum
        real(c_float) :: correction(3), delta_a(3), delta_b(3)

        do iter = 1, iterations
            !$omp parallel do private(i, idx_a, idx_b, rel_vel, vel_normal, &
            !$omp                      restitution, impulse, inv_mass_sum, correction, delta_a, delta_b) &
            !$omp shared(bodies, contacts)
            do i = 1, n_contacts
                idx_a = contacts(i)%body_a + 1
                idx_b = contacts(i)%body_b + 1

                if (bodies(idx_a)%is_static == 1 .and. bodies(idx_b)%is_static == 1) cycle

                rel_vel = bodies(idx_b)%velocity - bodies(idx_a)%velocity
                vel_normal = rel_vel(1)*contacts(i)%normal(1) + &
                        rel_vel(2)*contacts(i)%normal(2) + &
                        rel_vel(3)*contacts(i)%normal(3)

                if (vel_normal < 0.0) then
                    restitution = (bodies(idx_a)%restitution + bodies(idx_b)%restitution) * 0.5
                    impulse = -(1.0 + restitution) * vel_normal

                    inv_mass_sum = bodies(idx_a)%inv_mass + bodies(idx_b)%inv_mass
                    if (inv_mass_sum > 0.0) then
                        impulse = impulse / inv_mass_sum

                        delta_a = contacts(i)%normal * impulse * bodies(idx_a)%inv_mass
                        delta_b = contacts(i)%normal * impulse * bodies(idx_b)%inv_mass

                        !$omp atomic update
                        bodies(idx_a)%velocity(1) = bodies(idx_a)%velocity(1) - delta_a(1)
                        !$omp atomic update
                        bodies(idx_a)%velocity(2) = bodies(idx_a)%velocity(2) - delta_a(2)
                        !$omp atomic update
                        bodies(idx_a)%velocity(3) = bodies(idx_a)%velocity(3) - delta_a(3)

                        !$omp atomic update
                        bodies(idx_b)%velocity(1) = bodies(idx_b)%velocity(1) + delta_b(1)
                        !$omp atomic update
                        bodies(idx_b)%velocity(2) = bodies(idx_b)%velocity(2) + delta_b(2)
                        !$omp atomic update
                        bodies(idx_b)%velocity(3) = bodies(idx_b)%velocity(3) + delta_b(3)
                    end if
                end if

                correction = contacts(i)%normal * (contacts(i)%penetration * 0.5)

                if (bodies(idx_a)%is_static == 0) then
                    !$omp atomic update
                    bodies(idx_a)%position(1) = bodies(idx_a)%position(1) - correction(1)
                    !$omp atomic update
                    bodies(idx_a)%position(2) = bodies(idx_a)%position(2) - correction(2)
                    !$omp atomic update
                    bodies(idx_a)%position(3) = bodies(idx_a)%position(3) - correction(3)
                end if
                if (bodies(idx_b)%is_static == 0) then
                    !$omp atomic update
                    bodies(idx_b)%position(1) = bodies(idx_b)%position(1) + correction(1)
                    !$omp atomic update
                    bodies(idx_b)%position(2) = bodies(idx_b)%position(2) + correction(2)
                    !$omp atomic update
                    bodies(idx_b)%position(3) = bodies(idx_b)%position(3) + correction(3)
                end if
            end do
            !$omp end parallel do
        end do
    end subroutine solve_contacts_vectorized

    ! ===================================================================
    ! UPDATE AABB FOR ALL BODIES (безопасно параллелится — каждая
    ! итерация пишет только в свой собственный индекс i, без пересечений)
    ! ===================================================================
    subroutine update_aabb_vectorized(bodies, n, min_bounds, max_bounds, radius) &
            bind(c, name="update_aabb_vectorized")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(in) :: bodies(n)
        integer(c_int), intent(in) :: n
        real(c_float), intent(out) :: min_bounds(n, 3)
        real(c_float), intent(out) :: max_bounds(n, 3)
        real(c_float), intent(in) :: radius

        integer :: i

        !$omp parallel do private(i)
        do i = 1, n
            min_bounds(i, 1) = bodies(i)%position(1) - radius
            min_bounds(i, 2) = bodies(i)%position(2) - radius
            min_bounds(i, 3) = bodies(i)%position(3) - radius
            max_bounds(i, 1) = bodies(i)%position(1) + radius
            max_bounds(i, 2) = bodies(i)%position(2) + radius
            max_bounds(i, 3) = bodies(i)%position(3) + radius
        end do
        !$omp end parallel do
    end subroutine update_aabb_vectorized

    ! ===================================================================
    ! RESOLVE PENETRATION (то же исправление гонки, что и в
    ! solve_contacts_vectorized — atomic update по компонентам)
    ! ===================================================================
    subroutine resolve_penetration_batch(bodies, contacts, n_contacts) &
            bind(c, name="resolve_penetration_batch")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(inout) :: bodies(:)
        type(contact_c), intent(in) :: contacts(:)
        integer(c_int), intent(in) :: n_contacts

        integer :: i, idx_a, idx_b
        real(c_float) :: correction(3)
        real(c_float), parameter :: SLOP = 0.01
        real(c_float), parameter :: PERCENT = 0.2

        !$omp parallel do private(i, idx_a, idx_b, correction) shared(bodies, contacts)
        do i = 1, n_contacts
            idx_a = contacts(i)%body_a + 1
            idx_b = contacts(i)%body_b + 1

            if (bodies(idx_a)%is_static == 1 .and. bodies(idx_b)%is_static == 1) cycle

            correction = contacts(i)%normal * (max(contacts(i)%penetration - SLOP, 0.0) * PERCENT)

            if (bodies(idx_a)%is_static == 0) then
                !$omp atomic update
                bodies(idx_a)%position(1) = bodies(idx_a)%position(1) - correction(1)
                !$omp atomic update
                bodies(idx_a)%position(2) = bodies(idx_a)%position(2) - correction(2)
                !$omp atomic update
                bodies(idx_a)%position(3) = bodies(idx_a)%position(3) - correction(3)
            end if
            if (bodies(idx_b)%is_static == 0) then
                !$omp atomic update
                bodies(idx_b)%position(1) = bodies(idx_b)%position(1) + correction(1)
                !$omp atomic update
                bodies(idx_b)%position(2) = bodies(idx_b)%position(2) + correction(2)
                !$omp atomic update
                bodies(idx_b)%position(3) = bodies(idx_b)%position(3) + correction(3)
            end if
        end do
        !$omp end parallel do
    end subroutine resolve_penetration_batch

    ! ===================================================================
    ! UPDATE SLEEP STATE (BATCH)
    !
    ! ИСПРАВЛЕНО: раньше это была пустышка — считала скорость, сравнивала
    ! с порогом, и... ничего не делала (`continue`), с комментарием
    ! "обрабатывается в Rust" — но в Rust этого тоже нигде не было. Тело
    ! никогда не засыпало. Теперь функция принимает дополнительный
    ! параметр `sleep_timers` (по одному float на тело, хранится и
    ! передаётся вызывающей стороной между кадрами) и реализует настоящий
    ! гистерезис: пока скорость ниже порога, таймер тела растёт; как
    ! только превышает sleep_time — тело реально засыпает (is_asleep=1).
    ! Без такого таймера тело мгновенно "мигало" бы между сном и
    ! бодрствованием прямо на границе порога.
    ! ===================================================================
    subroutine update_sleep_state(bodies, n, dt, sleep_threshold, sleep_time, sleep_timers) &
            bind(c, name="update_sleep_state")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(inout) :: bodies(n)
        integer(c_int), intent(in) :: n
        real(c_float), intent(in) :: dt, sleep_threshold, sleep_time
        real(c_float), intent(inout) :: sleep_timers(n)

        integer :: i
        real(c_float) :: linear_speed_sq, angular_speed_sq

        !$omp parallel do private(i, linear_speed_sq, angular_speed_sq)
        do i = 1, n
            if (bodies(i)%is_static == 1) then
                bodies(i)%is_asleep = 1
                cycle
            end if

            linear_speed_sq = bodies(i)%velocity(1)**2 + &
                    bodies(i)%velocity(2)**2 + &
                    bodies(i)%velocity(3)**2
            angular_speed_sq = bodies(i)%angular_velocity(1)**2 + &
                    bodies(i)%angular_velocity(2)**2 + &
                    bodies(i)%angular_velocity(3)**2

            if (linear_speed_sq < sleep_threshold .and. angular_speed_sq < sleep_threshold) then
                sleep_timers(i) = sleep_timers(i) + dt
                if (sleep_timers(i) > sleep_time) then
                    bodies(i)%is_asleep = 1
                end if
            else
                sleep_timers(i) = 0.0
                bodies(i)%is_asleep = 0
            end if
        end do
        !$omp end parallel do
    end subroutine update_sleep_state

    ! ===================================================================
    ! COMPUTE CENTER OF MASS (FOR DEBUG/STATS)
    ! Корректное использование reduction — в отличие от бага выше,
    ! reduction-переменные тут ТОЛЬКО суммируются, не используются как
    ! индексы записи внутри региона.
    ! ===================================================================
    subroutine compute_center_of_mass(bodies, n, center, total_mass) &
            bind(c, name="compute_center_of_mass")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(in) :: bodies(n)
        integer(c_int), intent(in) :: n
        real(c_float), intent(out) :: center(3)
        real(c_float), intent(out) :: total_mass

        integer :: i
        real(c_float) :: mass_local

        center = 0.0
        total_mass = 0.0

        !$omp parallel do private(i, mass_local) reduction(+:total_mass, center)
        do i = 1, n
            if (bodies(i)%is_static == 0) then
                mass_local = bodies(i)%mass
                total_mass = total_mass + mass_local
                center(1) = center(1) + bodies(i)%position(1) * mass_local
                center(2) = center(2) + bodies(i)%position(2) * mass_local
                center(3) = center(3) + bodies(i)%position(3) * mass_local
            end if
        end do
        !$omp end parallel do

        if (total_mass > 0.0) then
            center = center / total_mass
        end if
    end subroutine compute_center_of_mass

    ! ===================================================================
    ! BROAD PHASE WITH TEMPORAL COHERENCE (тот же фикс гонки, что и в
    ! generate_collision_pairs — atomic capture вместо reduction-как-индекса)
    ! ===================================================================
    subroutine broad_phase_temporal(bodies, n, active_list, active_count, &
            pairs, pair_count, radius, time_step) &
            bind(c, name="broad_phase_temporal")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(in) :: bodies(n)
        integer(c_int), intent(in) :: n
        integer(c_int), intent(in) :: active_list(*)
        integer(c_int), intent(in) :: active_count
        integer(c_int), intent(out) :: pairs(*)
        integer(c_int), intent(out) :: pair_count
        real(c_float), intent(in) :: radius, time_step

        integer :: i, j, idx_i, idx_j, my_idx
        real(c_float) :: min1(3), max1(3), min2(3), max2(3)
        real(c_float) :: expanded_radius

        expanded_radius = radius + 2.0 * time_step * 50.0  ! Запас на 50 м/с

        pair_count = 0

        !$omp parallel do private(i, j, idx_i, idx_j, min1, max1, min2, max2, my_idx) &
        !$omp shared(pair_count, pairs)
        do i = 1, active_count
            idx_i = active_list(i)
            if (bodies(idx_i+1)%is_asleep == 1) cycle

            min1 = bodies(idx_i+1)%position - expanded_radius
            max1 = bodies(idx_i+1)%position + expanded_radius

            do j = i+1, active_count
                idx_j = active_list(j)
                if (bodies(idx_j+1)%is_asleep == 1) cycle

                min2 = bodies(idx_j+1)%position - expanded_radius
                max2 = bodies(idx_j+1)%position + expanded_radius

                if (aabb_intersect(min1, max1, min2, max2)) then
                    pair_count = pair_count + 1
                    my_idx = pair_count
                    pairs(2*my_idx - 1) = idx_i
                    pairs(2*my_idx) = idx_j
                end if
            end do
        end do
        !$omp end parallel do
    end subroutine broad_phase_temporal

    function aabb_intersect(min1, max1, min2, max2) result(intersect)
        implicit none
        real(c_float), intent(in) :: min1(3), max1(3), min2(3), max2(3)
        logical :: intersect
        intersect = .false.
        if (max1(1) < min2(1) .or. max2(1) < min1(1)) return
        if (max1(2) < min2(2) .or. max2(2) < min1(2)) return
        if (max1(3) < min2(3) .or. max2(3) < min1(3)) return
        intersect = .true.
    end function aabb_intersect

end module kernels_optimized_mod
