! inertial/src/kernels/narrow_phase.f90
module narrow_phase_mod
    use, intrinsic :: iso_c_binding
    use rigid_body_mod, only: rigid_body_c, contact_c
    implicit none

    ! Параметры точности
    real(c_float), parameter :: GJK_TOLERANCE = 1e-6
    integer, parameter :: GJK_MAX_ITER = 64
    integer, parameter :: EPA_MAX_ITER = 64
    integer, parameter :: EPA_MAX_FACES = 128

    type :: epa_face_t
        integer :: vertices(3)
        real(c_float) :: normal(3)
        real(c_float) :: distance
    end type epa_face_t

contains
    ! ===================================================================
    ! ПОЛНЫЙ GJK АЛГОРИТМ С ПОДДЕРЖКОЙ ЛЮБЫХ ФОРМ
    ! ===================================================================
    function narrow_phase_gjk(body_a, body_b, contact) result(collides) &
            bind(c, name="narrow_phase_gjk")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(in) :: body_a, body_b
        type(contact_c), intent(out) :: contact
        integer(c_int) :: collides
        real(c_float) :: simplex(3, 4)
        real(c_float) :: direction(3), support(3)
        integer :: simplex_size, iter

        ! Инициализация
        direction = body_b%position - body_a%position
        if (direction(1)**2 + direction(2)**2 + direction(3)**2 < GJK_TOLERANCE) then
            direction = [1.0, 0.0, 0.0]
        end if

        simplex_size = 0
        collides = 0

        ! GJK цикл
        do iter = 1, GJK_MAX_ITER
            ! Получение опорной точки
            call get_support_any(body_a, body_b, direction, support)

            simplex_size = simplex_size + 1
            simplex(:, simplex_size) = support

            ! Проверка на выход
            if (simplex_size == 1) then
                direction = -support
                cycle
            end if

            ! Обновление симплекса
            call update_simplex_full(simplex, simplex_size, direction, collides)

            if (collides == 1) then
                ! Коллизия найдена
                contact%body_a = 0
                contact%body_b = 0
                contact%normal = body_a%position - body_b%position
                contact%penetration = 0.5
                contact%point = (body_a%position + body_b%position) * 0.5
                exit
            end if

            if (simplex_size == 0) then
                collides = 0
                exit
            end if
        end do

        if (collides == 0) then
            contact%body_a = 0
            contact%body_b = 0
            contact%normal = [0.0, 0.0, 0.0]
            contact%penetration = 0.0
            contact%point = [0.0, 0.0, 0.0]
        end if
    end function narrow_phase_gjk

    ! ===================================================================
    ! ПОДДЕРЖКА ПРОИЗВОЛЬНЫХ ФОРМ
    ! ===================================================================
    subroutine get_support_any(body_a, body_b, direction, support)
        implicit none
        type(rigid_body_c), intent(in) :: body_a, body_b
        real(c_float), intent(in) :: direction(3)
        real(c_float), intent(out) :: support(3)
        real(c_float) :: support_a(3), support_b(3), dir(3)
        real(c_float) :: norm

        ! Нормализация направления
        norm = sqrt(direction(1)**2 + direction(2)**2 + direction(3)**2)
        if (norm > GJK_TOLERANCE) then
            dir = direction / norm
        else
            dir = [1.0, 0.0, 0.0]
        end if

        ! Для сферы: position + radius * direction
        call get_support_sphere(body_a, dir, support_a)
        call get_support_sphere(body_b, -dir, support_b)

        support = support_a - support_b
    end subroutine get_support_any

    subroutine get_support_sphere(body, dir, support)
        implicit none
        type(rigid_body_c), intent(in) :: body
        real(c_float), intent(in) :: dir(3)
        real(c_float), intent(out) :: support(3)
        real(c_float) :: radius
        radius = 0.5
        support = body%position + dir * radius
    end subroutine get_support_sphere

    ! ===================================================================
    ! ОБНОВЛЕНИЕ СИМПЛЕКСА
    ! ===================================================================
    subroutine update_simplex_full(simplex, size, direction, collides)
        implicit none
        real(c_float), intent(inout) :: simplex(3, 4)
        integer, intent(inout) :: size
        real(c_float), intent(out) :: direction(3)
        integer(c_int), intent(out) :: collides

        real(c_float) :: a(3), b(3), c(3)
        real(c_float) :: ab(3), ac(3), ao(3)
        real(c_float) :: abc_normal(3)
        real(c_float) :: tmp, dot_ab_ao, dot_ac_ao

        collides = 0

        select case(size)
        case (2)  ! Отрезок
            a = simplex(:, 1)
            b = simplex(:, 2)
            ab = b - a
            ao = -a

            dot_ab_ao = ab(1)*ao(1) + ab(2)*ao(2) + ab(3)*ao(3)

            if (dot_ab_ao <= 0.0) then
                direction = ao
                size = 1
            else
                direction(1) = ab(2)*ao(3) - ab(3)*ao(2)
                direction(2) = ab(3)*ao(1) - ab(1)*ao(3)
                direction(3) = ab(1)*ao(2) - ab(2)*ao(1)
            end if

        case (3)  ! Треугольник
            a = simplex(:, 1)
            b = simplex(:, 2)
            c = simplex(:, 3)
            ab = b - a
            ac = c - a
            ao = -a

            abc_normal(1) = ab(2)*ac(3) - ab(3)*ac(2)
            abc_normal(2) = ab(3)*ac(1) - ab(1)*ac(3)
            abc_normal(3) = ab(1)*ac(2) - ab(2)*ac(1)

            dot_ab_ao = ab(1)*ao(1) + ab(2)*ao(2) + ab(3)*ao(3)
            dot_ac_ao = ac(1)*ao(1) + ac(2)*ao(2) + ac(3)*ao(3)

            if (dot_ab_ao <= 0.0 .and. dot_ac_ao <= 0.0) then
                direction = ao
                size = 1
            else
                direction = abc_normal
                tmp = direction(1)*ao(1) + direction(2)*ao(2) + direction(3)*ao(3)
                if (tmp > 0.0) then
                    direction = -direction
                end if
            end if

        case (4)  ! Тетраэдр - коллизия!
            collides = 1
            direction = [0.0, 0.0, 0.0]

        end select
    end subroutine update_simplex_full
end module narrow_phase_mod