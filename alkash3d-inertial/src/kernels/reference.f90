! inertial/src/kernels/reference.f90
!
! ДОБАВЛЕНО: integrate_bodies, solve_contacts и update_aabb были объявлены
! как extern "C" в ffi/mod.rs (и используются методами
! FortranPhysics::integrate/solve_contacts/update_bounds), но нигде не
! были реализованы ни в одном из .f90 файлов — только их "_vectorized"/
! "batch"-варианты существовали. При сборке это привело бы к ошибке
! линковщика "undefined reference to integrate_bodies" (и т.д.), как
! только компоновщик попытался бы связать символы, на которые ссылается
! Rust-код (даже если они реально не вызываются в рантайме — сама ссылка
! уже требует существования символа). Ниже — простые, однопоточные,
! эталонные версии: без OpenMP, для отладки/сверки с оптимизированными
! аналогами и чтобы всё МОГЛО собраться.
module reference_mod
    use, intrinsic :: iso_c_binding
    use rigid_body_mod, only: rigid_body_c, contact_c
    implicit none

contains
    subroutine integrate_bodies(bodies, n, dt) bind(c, name="integrate_bodies")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(inout) :: bodies(n)
        integer(c_int), intent(in) :: n
        real(c_float), intent(in) :: dt
        integer :: i

        do i = 1, n
            if (bodies(i)%is_asleep == 0 .and. bodies(i)%is_static == 0) then
                bodies(i)%position = bodies(i)%position + bodies(i)%velocity * dt
                bodies(i)%angular_velocity = bodies(i)%angular_velocity + &
                        bodies(i)%angular_acceleration * dt
                bodies(i)%velocity = bodies(i)%velocity * (1.0 - bodies(i)%linear_damping * dt)
                bodies(i)%angular_velocity = bodies(i)%angular_velocity * &
                        (1.0 - bodies(i)%angular_damping * dt)
            end if
        end do
    end subroutine integrate_bodies

    subroutine solve_contacts(bodies, contacts, n_contacts, iterations) &
            bind(c, name="solve_contacts")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(inout) :: bodies(:)
        type(contact_c), intent(inout) :: contacts(:)
        integer(c_int), intent(in) :: n_contacts, iterations

        integer :: iter, i, idx_a, idx_b
        real(c_float) :: rel_vel(3), vel_normal, impulse, restitution, inv_mass_sum
        real(c_float) :: correction(3)

        do iter = 1, iterations
            do i = 1, n_contacts
                idx_a = contacts(i)%body_a + 1
                idx_b = contacts(i)%body_b + 1
                if (bodies(idx_a)%is_static == 1 .and. bodies(idx_b)%is_static == 1) cycle

                rel_vel = bodies(idx_b)%velocity - bodies(idx_a)%velocity
                vel_normal = rel_vel(1)*contacts(i)%normal(1) + &
                        rel_vel(2)*contacts(i)%normal(2) + rel_vel(3)*contacts(i)%normal(3)

                if (vel_normal < 0.0) then
                    restitution = (bodies(idx_a)%restitution + bodies(idx_b)%restitution) * 0.5
                    inv_mass_sum = bodies(idx_a)%inv_mass + bodies(idx_b)%inv_mass
                    if (inv_mass_sum > 0.0) then
                        impulse = -(1.0 + restitution) * vel_normal / inv_mass_sum
                        bodies(idx_a)%velocity = bodies(idx_a)%velocity - &
                                contacts(i)%normal * impulse * bodies(idx_a)%inv_mass
                        bodies(idx_b)%velocity = bodies(idx_b)%velocity + &
                                contacts(i)%normal * impulse * bodies(idx_b)%inv_mass
                    end if
                end if

                ! Корректная покомпонентная коррекция позиции (не баг из
                ! Rust-версии lib.rs, где по ошибке бралась только normal(1)
                ! и применялась ко всем трём осям сразу).
                correction = contacts(i)%normal * (contacts(i)%penetration * 0.5)
                if (bodies(idx_a)%is_static == 0) then
                    bodies(idx_a)%position = bodies(idx_a)%position - correction
                end if
                if (bodies(idx_b)%is_static == 0) then
                    bodies(idx_b)%position = bodies(idx_b)%position + correction
                end if
            end do
        end do
    end subroutine solve_contacts

    subroutine update_aabb(bodies, n, min_bounds, max_bounds, radius) &
            bind(c, name="update_aabb")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(in) :: bodies(n)
        integer(c_int), intent(in) :: n
        real(c_float), intent(out) :: min_bounds(n, 3)
        real(c_float), intent(out) :: max_bounds(n, 3)
        real(c_float), intent(in) :: radius
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
end module reference_mod
