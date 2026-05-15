! inertial/src/kernels/solver.f90
module solver_mod
    use, intrinsic :: iso_c_binding
    use rigid_body_mod, only: rigid_body_c, constraint_c
    implicit none

contains
    subroutine solve_constraints(bodies, constraints, n_constraints, iterations) &
            bind(c, name="solve_constraints")
        type(rigid_body_c), intent(inout) :: bodies(:)
        type(constraint_c), intent(inout) :: constraints(:)
        integer(c_int), intent(in) :: n_constraints, iterations
        integer :: iter, i

        do iter = 1, iterations
            do i = 1, n_constraints
                call solve_ball_joint(bodies(constraints(i)%body_a+1), &
                        bodies(constraints(i)%body_b+1), &
                        constraints(i))
            end do
        end do
    end subroutine solve_constraints

    subroutine solve_ball_joint(body_a, body_b, constraint)
        type(rigid_body_c), intent(inout) :: body_a, body_b
        type(constraint_c), intent(inout) :: constraint
        real(c_float) :: ra(3), rb(3), c(3), impulse, effective_mass, bias

        ra = constraint%anchor_a - body_a%position
        rb = constraint%anchor_b - body_b%position
        c = (body_a%position + ra) - (body_b%position + rb)
        effective_mass = 1.0 / (body_a%inv_mass + body_b%inv_mass + 0.001)
        bias = constraint%bias * 0.2
        impulse = effective_mass * (-c(1) * bias - constraint%accumulated_impulse)
        body_a%velocity(1) = body_a%velocity(1) + impulse * body_a%inv_mass
        body_b%velocity(1) = body_b%velocity(1) - impulse * body_b%inv_mass
        constraint%accumulated_impulse = constraint%accumulated_impulse + impulse
    end subroutine solve_ball_joint
end module solver_mod