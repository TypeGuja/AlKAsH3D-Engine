! inertial/src/kernels/narrow_phase.f90
module narrow_phase_mod
    use, intrinsic :: iso_c_binding
    implicit none

    type, bind(c) :: contact_c
        integer(c_int) :: body_a
        integer(c_int) :: body_b
        real(c_float) :: normal(3)
        real(c_float) :: penetration
        real(c_float) :: point(3)
    end type contact_c

contains
    ! GJK (Gilbert-Johnson-Keerthi) алгоритм для检测 коллизий
    function narrow_phase_gjk(body_a, body_b, contact) result(collides) &
            bind(c, name="narrow_phase_gjk")
        use rigid_body_mod, only: rigid_body_c
        type(rigid_body_c), intent(in) :: body_a, body_b
        type(contact_c), intent(out) :: contact
        integer(c_int) :: collides

        real(c_float) :: simplex(3, 4)  ! Поддержка до 4 точек
        integer :: simplex_size
        real(c_float) :: direction(3), support_a(3), support_b(3), support(3)
        real(c_float) :: v(3), v_dot, prev_v_dot
        integer :: iterations, max_iterations = 32
        logical :: found

        ! Начальное направление - от центра A к центру B
        direction = body_b%position - body_a%position
        if (direction(1) == 0.0 .and. direction(2) == 0.0 .and. direction(3) == 0.0) then
            direction = [1.0, 0.0, 0.0]
        end if

        simplex_size = 0
        collides = 0
        iterations = 0

        do while (iterations < max_iterations)
            ! Получаем опорную точку в направлении direction
            call get_support(body_a, body_b, direction, support)

            ! Добавляем в симплекс
            simplex_size = simplex_size + 1
            simplex(:, simplex_size) = support

            if (simplex_size == 1) then
                direction = -support
                iterations = iterations + 1
                cycle
            end if

            ! Проверяем, прошли ли через начало координат
            v = -support
            v_dot = v(1)*direction(1) + v(2)*direction(2) + v(3)*direction(3)

            if (v_dot <= 0.0) then
                ! Нет коллизии
                collides = 0
                return
            end if

            ! Обновляем симплекс (алгоритм поиска ближайшей точки)
            call update_simplex(simplex, simplex_size, direction, found)

            if (found) then
                collides = 1

                ! Вычисляем контактную информацию
                call compute_contact_info(body_a, body_b, simplex, simplex_size, contact)
                return
            end if

            iterations = iterations + 1
        end do

        ! Если дошли до максимального числа итераций - считаем что коллизия есть
        collides = 1
        contact%body_a = 0
        contact%body_b = 0
        contact%normal = [0.0, 0.0, 0.0]
        contact%penetration = 0.0
        contact%point = [0.0, 0.0, 0.0]
    end function narrow_phase_gjk

    subroutine get_support(body_a, body_b, direction, support)
        use rigid_body_mod, only: rigid_body_c
        type(rigid_body_c), intent(in) :: body_a, body_b
        real(c_float), intent(in) :: direction(3)
        real(c_float), intent(out) :: support(3)

        real(c_float) :: support_a(3), support_b(3)
        real(c_float) :: dir_norm(3), norm

        ! Нормализуем направление
        norm = sqrt(direction(1)**2 + direction(2)**2 + direction(3)**2)
        if (norm > 0.0) then
            dir_norm = direction / norm
        else
            dir_norm = [1.0, 0.0, 0.0]
        end if

        ! Опорная точка на теле A в направлении dir_norm
        support_a = body_a%position + dir_norm * 0.5

        ! Опорная точка на теле B в направлении -dir_norm
        support_b = body_b%position - dir_norm * 0.5

        ! Точка Минковского
        support = support_a - support_b
    end subroutine get_support

    subroutine update_simplex(simplex, size, direction, found)
        real(c_float), intent(inout) :: simplex(3, 4)
        integer, intent(inout) :: size
        real(c_float), intent(out) :: direction(3)
        logical, intent(out) :: found

        real(c_float) :: a(3), b(3), c(3), d(3)
        real(c_float) :: ab(3), ac(3), ao(3), abc_normal(3)
        real(c_float) :: dot_ab_ao, dot_ac_ao, dot_abc_ao

        found = .false.

        select case(size)
        case (2)
            ! Линия: проверяем ближайшую точку к началу
            a = simplex(:, 1)
            b = simplex(:, 2)
            ab = b - a
            ao = -a

            dot_ab_ao = ab(1)*ao(1) + ab(2)*ao(2) + ab(3)*ao(3)

            if (dot_ab_ao <= 0.0) then
                ! Ближайшая точка - A
                direction = ao
                size = 1
            else
                ! Ближайшая точка на отрезке
                direction = cross_product(ab, cross_product(ab, ao))
                if (direction(1) == 0.0 .and. direction(2) == 0.0 .and. direction(3) == 0.0) then
                    direction = ao
                end if
            end if

        case (3)
            ! Треугольник: проверяем регионы Вороного
            a = simplex(:, 1)
            b = simplex(:, 2)
            c = simplex(:, 3)
            ab = b - a
            ac = c - a
            ao = -a

            abc_normal = cross_product(ab, ac)
            dot_abc_ao = abc_normal(1)*ao(1) + abc_normal(2)*ao(2) + abc_normal(3)*ao(3)

            if (dot_abc_ao > 0.0) then
                ! Над треугольником
                direction = abc_normal
            else
                ! Под треугольником
                direction = -abc_normal
            end if

        case (4)
            ! Тетраэдр: начало внутри - коллизия найдена
            found = .true.
            direction = [0.0, 0.0, 0.0]

        end select
    end subroutine update_simplex

    subroutine compute_contact_info(body_a, body_b, simplex, size, contact)
        use rigid_body_mod, only: rigid_body_c
        type(rigid_body_c), intent(in) :: body_a, body_b
        real(c_float), intent(in) :: simplex(3, 4)
        integer, intent(in) :: size
        type(contact_c), intent(out) :: contact

        real(c_float) :: closest_point(3), normal(3), penetration

        ! Находим ближайшую точку к началу в симплексе
        call find_closest_point(simplex, size, closest_point)

        ! Нормаль от B к A
        normal = body_a%position - body_b%position
        penetration = 0.5  ! Временное значение

        contact%body_a = 0
        contact%body_b = 0
        contact%normal = normal / sqrt(normal(1)**2 + normal(2)**2 + normal(3)**2)
        contact%penetration = penetration
        contact%point = closest_point
    end subroutine compute_contact_info

    subroutine find_closest_point(simplex, size, closest_point)
        real(c_float), intent(in) :: simplex(3, 4)
        integer, intent(in) :: size
        real(c_float), intent(out) :: closest_point(3)

        integer :: i

        closest_point = [0.0, 0.0, 0.0]

        do i = 1, size
            closest_point = closest_point + simplex(:, i)
        end do

        if (size > 0) then
            closest_point = closest_point / real(size, 8)
        end if
    end subroutine find_closest_point

    function cross_product(a, b) result(c)
        real(c_float), intent(in) :: a(3), b(3)
        real(c_float) :: c(3)

        c(1) = a(2)*b(3) - a(3)*b(2)
        c(2) = a(3)*b(1) - a(1)*b(3)
        c(3) = a(1)*b(2) - a(2)*b(1)
    end function cross_product
end module narrow_phase_mod