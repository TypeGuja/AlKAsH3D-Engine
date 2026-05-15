! inertial/src/kernels/broad_phase.f90
module broad_phase_mod
    use, intrinsic :: iso_c_binding
    use rigid_body_mod, only: rigid_body_c
    implicit none

contains
    subroutine broad_phase_sap(bodies, n, pairs, pair_count) bind(c, name="broad_phase_sap")
        type(rigid_body_c), intent(in) :: bodies(n)
        integer(c_int), intent(in) :: n
        integer(c_int), intent(out) :: pairs(*)
        integer(c_int), intent(out) :: pair_count
        integer :: i, j, count
        real(c_float) :: min1(3), max1(3), min2(3), max2(3)
        real(c_float) :: radius = 0.6

        count = 0

        do i = 1, n
            if (bodies(i)%is_asleep == 1) cycle
            min1 = bodies(i)%position - radius
            max1 = bodies(i)%position + radius
            do j = i+1, n
                if (bodies(j)%is_asleep == 1) cycle
                min2 = bodies(j)%position - radius
                max2 = bodies(j)%position + radius
                if (aabb_intersect(min1, max1, min2, max2)) then
                    count = count + 1
                    pairs(2*count - 1) = i - 1
                    pairs(2*count) = j - 1
                end if
            end do
        end do
        pair_count = count
    end subroutine broad_phase_sap

    function aabb_intersect(min1, max1, min2, max2) result(intersect)
        real(c_float), intent(in) :: min1(3), max1(3), min2(3), max2(3)
        logical :: intersect
        intersect = .false.
        if (max1(1) < min2(1) .or. max2(1) < min1(1)) return
        if (max1(2) < min2(2) .or. max2(2) < min1(2)) return
        if (max1(3) < min2(3) .or. max2(3) < min1(3)) return
        intersect = .true.
    end function aabb_intersect
end module broad_phase_mod