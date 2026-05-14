! inertial/src/kernels/broad_phase.f90
module broad_phase_mod
    use, intrinsic :: iso_c_binding
    implicit none

    type, bind(c) :: aabb_c
        real(c_float) :: min(3)
        real(c_float) :: max(3)
    end type aabb_c

contains
    ! Sweep and Prune (SAP) - быстрый broad phase для большого количества объектов
    subroutine broad_phase_sap(bodies, n, pairs, pair_count) bind(c, name="broad_phase_sap")
        use rigid_body_mod, only: rigid_body_c
        type(rigid_body_c), intent(in) :: bodies(n)
        integer(c_int), intent(in) :: n
        integer(c_int), intent(out) :: pairs(*)
        integer(c_int), intent(out) :: pair_count

        integer :: i, j, count
        real(c_float) :: aabb_min(3), aabb_max(3)
        real(c_float) :: margin = 0.1

        count = 0

        ! Простая O(n²) для начала (потом оптимизируем с сортировкой)
        do i = 1, n
            if (bodies(i)%is_asleep == 1) cycle

            ! Вычисляем AABB для тела i
            aabb_min = bodies(i)%position - 0.5
            aabb_max = bodies(i)%position + 0.5

            do j = i+1, n
                if (bodies(j)%is_asleep == 1) cycle

                if (aabb_intersect(aabb_min, aabb_max, bodies(j)%position - 0.5, bodies(j)%position + 0.5)) then
                    count = count + 1
                    pairs(count) = i - 1
                    pairs(count + 1) = j - 1
                end if
            end do
        end do

        pair_count = count
    end subroutine broad_phase_sap

    ! Оптимизированный SAP с сортировкой по оси X
    subroutine broad_phase_sap_optimized(bodies, n, pairs, pair_count) &
            bind(c, name="broad_phase_sap_optimized")
        use rigid_body_mod, only: rigid_body_c
        type(rigid_body_c), intent(in) :: bodies(n)
        integer(c_int), intent(in) :: n
        integer(c_int), intent(out) :: pairs(*)
        integer(c_int), intent(out) :: pair_count

        integer :: i, j, k, count
        integer, allocatable :: indices(:)
        real(c_float), allocatable :: min_x(:)
        real(c_float) :: aabb_min(3), aabb_max(3)
        real(c_float) :: margin = 0.1

        ! Создаём массив индексов
        allocate(indices(n))
        allocate(min_x(n))

        do i = 1, n
            indices(i) = i
            min_x(i) = bodies(i)%position(1) - 0.5
        end do

        ! Сортируем по min_x (пузырьком для простоты, позже заменить на quicksort)
        do i = 1, n-1
            do j = 1, n-i
                if (min_x(indices(j)) > min_x(indices(j+1))) then
                    k = indices(j)
                    indices(j) = indices(j+1)
                    indices(j+1) = k
                end if
            end do
        end do

        count = 0

        do i = 1, n
            k = indices(i)
            if (bodies(k)%is_asleep == 1) cycle

            aabb_min = bodies(k)%position - 0.5
            aabb_max = bodies(k)%position + 0.5

            do j = i+1, n
                if (min_x(indices(j)) > aabb_max(1)) exit

                if (aabb_intersect(aabb_min, aabb_max, &
                        bodies(indices(j))%position - 0.5, &
                        bodies(indices(j))%position + 0.5)) then
                    count = count + 1
                    pairs(2*count - 1) = k - 1
                    pairs(2*count) = indices(j) - 1
                end if
            end do
        end do

        pair_count = count
        deallocate(indices, min_x)
    end subroutine broad_phase_sap_optimized

    function aabb_intersect(min1, max1, min2, max2) result(intersect)
        real(c_float), intent(in) :: min1(3), max1(3), min2(3), max2(3)
        logical :: intersect

        intersect = .false.
        if (max1(1) < min2(1) .or. max2(1) < min1(1)) return
        if (max1(2) < min2(2) .or. max2(2) < min1(2)) return
        if (max1(3) < min2(3) .or. max2(3) < min1(3)) return
        intersect = .true.
    end function aabb_intersect

    ! Обновление AABB для всех тел
    subroutine update_aabb(bodies, n, min_bounds, max_bounds) bind(c, name="update_aabb")
        use rigid_body_mod, only: rigid_body_c
        type(rigid_body_c), intent(in) :: bodies(n)
        integer(c_int), intent(in) :: n
        real(c_float), intent(out) :: min_bounds(n, 3)
        real(c_float), intent(out) :: max_bounds(n, 3)

        integer :: i
        real(c_float) :: radius = 0.5

        do i = 1, n
            min_bounds(i, 1) = bodies(i)%position(1) - radius
            min_bounds(i, 2) = bodies(i)%position(2) - radius
            min_bounds(i, 3) = bodies(i)%position(3) - radius

            max_bounds(i, 1) = bodies(i)%position(1) + radius
            max_bounds(i, 2) = bodies(i)%position(2) + radius
            max_bounds(i, 3) = bodies(i)%position(3) + radius
        end do
    end subroutine update_aabb
end module broad_phase_mod