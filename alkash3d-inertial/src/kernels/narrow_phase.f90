! inertial/src/kernels/narrow_phase.f90
module narrow_phase_mod
    use, intrinsic :: iso_c_binding
    use rigid_body_mod, only: rigid_body_c, contact_c
    implicit none

contains
    function narrow_phase_gjk(body_a, body_b, contact) result(collides) &
            bind(c, name="narrow_phase_gjk")
        type(rigid_body_c), intent(in) :: body_a, body_b
        type(contact_c), intent(out) :: contact
        integer(c_int) :: collides
        real(c_float) :: simplex(3, 4), direction(3), support(3), ao(3)
        integer :: simplex_size, iterations
        integer, parameter :: max_iterations = 32

        direction = body_b%position - body_a%position
        if (direction(1)**2 + direction(2)**2 + direction(3)**2 < 1e-12) then
            direction = [1.0, 0.0, 0.0]
        end if

        simplex_size = 0
        collides = 0

        do iterations = 1, max_iterations
            call get_support(body_a, body_b, direction, support)
            simplex_size = simplex_size + 1
            simplex(:, simplex_size) = support
            if (simplex_size == 1) then
                direction = -support
                cycle
            end if
            ao = -simplex(:, 1)
            if (ao(1)*direction(1) + ao(2)*direction(2) + ao(3)*direction(3) <= 0.0) then
                collides = 0
                return
            end if
            call update_simplex(simplex, simplex_size, direction, collides)
            if (collides == 1) exit
        end do

        if (collides == 1) then
            call compute_contact_info(body_a, body_b, simplex, simplex_size, contact)
        else
            contact%body_a = 0
            contact%body_b = 0
            contact%normal = [0.0, 0.0, 0.0]
            contact%penetration = 0.0
            contact%point = [0.0, 0.0, 0.0]
        end if
    end function narrow_phase_gjk

    subroutine get_support(body_a, body_b, direction, support)
        type(rigid_body_c), intent(in) :: body_a, body_b
        real(c_float), intent(in) :: direction(3)
        real(c_float), intent(out) :: support(3)
        real(c_float) :: norm, dir_norm(3), radius
        radius = 0.5
        norm = sqrt(direction(1)**2 + direction(2)**2 + direction(3)**2)
        if (norm > 0.0) then
            dir_norm = direction / norm
        else
            dir_norm = [1.0, 0.0, 0.0]
        end if
        support = (body_a%position + dir_norm * radius) - (body_b%position - dir_norm * radius)
    end subroutine get_support

    subroutine update_simplex(simplex, size, direction, collides)
        real(c_float), intent(inout) :: simplex(3, 4)
        integer, intent(inout) :: size
        real(c_float), intent(out) :: direction(3)
        integer(c_int), intent(out) :: collides
        real(c_float) :: a(3), b(3), ab(3), ao(3), dot_ab_ao
        collides = 0
        select case(size)
        case (2)
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
                if (direction(1)**2 + direction(2)**2 + direction(3)**2 < 1e-12) then
                    direction = ao
                end if
            end if
        case (3)
            size = 2
            simplex(:, 2) = simplex(:, 3)
            direction = simplex(:, 1) - simplex(:, 2)
        case (4)
            collides = 1
            direction = [0.0, 0.0, 0.0]
        end select
    end subroutine update_simplex

    subroutine compute_contact_info(body_a, body_b, simplex, size, contact)
        type(rigid_body_c), intent(in) :: body_a, body_b
        real(c_float), intent(in) :: simplex(3, 4)
        integer, intent(in) :: size
        type(contact_c), intent(out) :: contact
        real(c_float) :: closest(3), normal(3), norm
        integer :: i
        closest = [0.0, 0.0, 0.0]
        do i = 1, size
            closest = closest + simplex(:, i)
        end do
        if (size > 0) closest = closest / real(size)
        normal = body_a%position - body_b%position
        norm = sqrt(normal(1)**2 + normal(2)**2 + normal(3)**2)
        if (norm > 0.0) normal = normal / norm
        contact%body_a = 0
        contact%body_b = 0
        contact%normal = normal
        contact%penetration = 0.5
        contact%point = closest
    end subroutine compute_contact_info
end module narrow_phase_mod