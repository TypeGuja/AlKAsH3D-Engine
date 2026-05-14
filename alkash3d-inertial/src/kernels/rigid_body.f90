! inertial/src/kernels/rigid_body.f90
module rigid_body_mod
    use, intrinsic :: iso_c_binding
    implicit none

    type, bind(c) :: rigid_body_c
        real(c_float) :: position(3)
        real(c_float) :: velocity(3)
        real(c_float) :: acceleration(3)
        real(c_float) :: angular_velocity(3)
        real(c_float) :: mass
        real(c_float) :: inv_mass
        real(c_float) :: restitution
        real(c_float) :: friction
        integer(c_int) :: is_static
        integer(c_int) :: is_asleep
    end type rigid_body_c

    ! ОПРЕДЕЛЯЕМ ТИП ЗДЕСЬ (до использования)
    type, bind(c) :: contact_c
        integer(c_int) :: body_a
        integer(c_int) :: body_b
        real(c_float) :: normal(3)
        real(c_float) :: penetration
        real(c_float) :: point(3)
    end type contact_c

contains
    subroutine integrate_bodies(bodies, n, dt) bind(c, name="integrate_bodies")
        type(rigid_body_c), intent(inout) :: bodies(n)
        integer(c_int), intent(in) :: n
        real(c_float), intent(in) :: dt

        integer :: i

        !$omp parallel do simd schedule(static)
        do i = 1, n
            if (bodies(i)%is_asleep == 0 .and. bodies(i)%is_static == 0) then
                bodies(i)%velocity = bodies(i)%velocity + bodies(i)%acceleration * dt
                bodies(i)%position = bodies(i)%position + bodies(i)%velocity * dt
            end if
        end do
        !$omp end parallel do simd
    end subroutine integrate_bodies

    subroutine solve_contacts(bodies, contacts, n_contacts, iterations) &
            bind(c, name="solve_contacts")

        type(rigid_body_c), intent(inout) :: bodies(:)
        type(contact_c), intent(inout) :: contacts(:)
        integer(c_int), intent(in) :: n_contacts, iterations

        integer :: iter, i

        do iter = 1, iterations
            do i = 1, n_contacts
                call resolve_contact(bodies(contacts(i)%body_a+1), &
                        bodies(contacts(i)%body_b+1), &
                        contacts(i)%normal, &
                        contacts(i)%penetration)
            end do
        end do
    end subroutine solve_contacts

    subroutine resolve_contact(a, b, normal, penetration)
        type(rigid_body_c), intent(inout) :: a, b
        real(c_float), intent(in) :: normal(3)
        real(c_float), intent(in) :: penetration

        real(c_float) :: rel_vel(3), vel_along, impulse, restitution, inv_mass_sum
        real(c_float) :: impulse_vec(3), correction(3)

        rel_vel = b%velocity - a%velocity
        vel_along = rel_vel(1)*normal(1) + rel_vel(2)*normal(2) + rel_vel(3)*normal(3)

        if (vel_along < 0.0) then
            restitution = (a%restitution + b%restitution) * 0.5
            impulse = -(1.0 + restitution) * vel_along
            inv_mass_sum = a%inv_mass + b%inv_mass
            impulse = impulse / inv_mass_sum

            impulse_vec = normal * impulse

            a%velocity = a%velocity - impulse_vec * a%inv_mass
            b%velocity = b%velocity + impulse_vec * b%inv_mass
        end if

        correction = normal * (penetration * 0.5)
        a%position = a%position - correction
        b%position = b%position + correction
    end subroutine resolve_contact
end module rigid_body_mod