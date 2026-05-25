! inertial/src/kernels/broad_phase.f90
module broad_phase_mod
    use, intrinsic :: iso_c_binding
    use rigid_body_mod, only: rigid_body_c
    implicit none

    type, bind(c) :: grid_cell_t
        integer(c_int) :: start_idx
        integer(c_int) :: count
    end type grid_cell_t

contains
    ! ===================================================================
    ! UNIFORM GRID BROAD PHASE - O(N) сложность
    ! ===================================================================
    subroutine broad_phase_grid(bodies, n, cell_size, grid_width, grid_height, &
            cell_starts, cell_counts, cell_pairs, pair_count) &
            bind(c, name="broad_phase_grid")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(in) :: bodies(n)
        integer(c_int), intent(in) :: n
        real(c_float), intent(in) :: cell_size
        integer(c_int), intent(in) :: grid_width, grid_height
        integer(c_int), intent(inout) :: cell_starts(grid_width, grid_height)
        integer(c_int), intent(inout) :: cell_counts(grid_width, grid_height)
        integer(c_int), intent(out) :: cell_pairs(*)
        integer(c_int), intent(out) :: pair_count

        integer :: i, j, k, x, y, cx, cy, start, count, idx
        integer :: neighbors(8)
        real(c_float) :: radius, min_x, max_x, min_z, max_z
        real(c_float) :: pos_x, pos_z

        radius = 0.6
        pair_count = 0

        ! Сброс счётчиков ячеек
        do y = 1, grid_height
            do x = 1, grid_width
                cell_starts(x, y) = 0
                cell_counts(x, y) = 0
            end do
        end do

        ! Проход 1: Подсчёт объектов в каждой ячейке
        do i = 1, n
            if (bodies(i)%is_asleep == 1) cycle

            pos_x = bodies(i)%position(1)
            pos_z = bodies(i)%position(3)

            x = floor(pos_x / cell_size) + 1
            y = floor(pos_z / cell_size) + 1

            if (x >= 1 .and. x <= grid_width .and. &
                    y >= 1 .and. y <= grid_height) then
                cell_counts(x, y) = cell_counts(x, y) + 1
            end if
        end do

        ! Проход 2: Префиксные суммы для индексов
        idx = 0
        do y = 1, grid_height
            do x = 1, grid_width
                cell_starts(x, y) = idx
                idx = idx + cell_counts(x, y)
            end do
        end do

        ! Сохраняем оригинальные старты для вставки
        do y = 1, grid_height
            do x = 1, grid_width
                cell_starts(x, y) = cell_starts(x, y) + 1
            end do
        end do

        ! Проход 3: Заполнение ячеек объектами
        do i = 1, n
            if (bodies(i)%is_asleep == 1) cycle

            pos_x = bodies(i)%position(1)
            pos_z = bodies(i)%position(3)

            x = floor(pos_x / cell_size) + 1
            y = floor(pos_z / cell_size) + 1

            if (x >= 1 .and. x <= grid_width .and. &
                    y >= 1 .and. y <= grid_height) then
                cell_starts(x, y) = cell_starts(x, y) - 1
                cell_pairs(cell_starts(x, y)) = i - 1
            end if
        end do

        ! Восстанавливаем старты
        do y = 1, grid_height
            do x = 1, grid_width
                cell_starts(x, y) = cell_starts(x, y) + 1
            end do
        end do

        ! Проход 4: Поиск коллизий (только соседние ячейки)
        idx = 1
        pair_count = 0

        do y = 1, grid_height
            do x = 1, grid_width
                ! Текущая ячейка
                start = cell_starts(x, y)
                count = cell_counts(x, y)

                ! Проверка внутри ячейки
                do i = start, start + count - 1
                    do j = i + 1, start + count - 1
                        cell_pairs(idx) = cell_pairs(i)
                        cell_pairs(idx + 1) = cell_pairs(j)
                        idx = idx + 2
                        pair_count = pair_count + 1
                    end do
                end do

                ! Проверка с соседними ячейками
                neighbors = [x+1, y, x, y+1, x+1, y+1, x-1, y+1]
                do k = 1, 7, 2
                    cx = neighbors(k)
                    cy = neighbors(k+1)
                    if (cx >= 1 .and. cx <= grid_width .and. &
                            cy >= 1 .and. cy <= grid_height) then

                        start = cell_starts(cx, cy)
                        count = cell_counts(cx, cy)

                        do i = cell_starts(x, y), cell_starts(x, y) + cell_counts(x, y) - 1
                            do j = start, start + count - 1
                                cell_pairs(idx) = cell_pairs(i)
                                cell_pairs(idx + 1) = cell_pairs(j)
                                idx = idx + 2
                                pair_count = pair_count + 1
                            end do
                        end do
                    end if
                end do
            end do
        end do
    end subroutine broad_phase_grid

    ! ===================================================================
    ! ОПТИМИЗИРОВАННЫЙ SAP С АКТИВНЫМ СПИСКОМ
    ! ===================================================================
    subroutine broad_phase_sap_optimized(bodies, n, active_indices, active_count, &
            pairs, pair_count) &
            bind(c, name="broad_phase_sap_optimized")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(in) :: bodies(n)
        integer(c_int), intent(in) :: n
        integer(c_int), intent(in) :: active_indices(*)
        integer(c_int), intent(in) :: active_count
        integer(c_int), intent(out) :: pairs(*)
        integer(c_int), intent(out) :: pair_count

        integer :: i, j, idx_i, idx_j
        real(c_float) :: min1(3), max1(3), min2(3), max2(3)
        real(c_float) :: radius
        radius = 0.6

        pair_count = 0

        do i = 1, active_count
            idx_i = active_indices(i)
            if (bodies(idx_i+1)%is_asleep == 1) cycle

            min1 = bodies(idx_i+1)%position - radius
            max1 = bodies(idx_i+1)%position + radius

            do j = i+1, active_count
                idx_j = active_indices(j)
                if (bodies(idx_j+1)%is_asleep == 1) cycle

                min2 = bodies(idx_j+1)%position - radius
                max2 = bodies(idx_j+1)%position + radius

                if (aabb_intersect(min1, max1, min2, max2)) then
                    pair_count = pair_count + 1
                    pairs(2*pair_count - 1) = idx_i
                    pairs(2*pair_count) = idx_j
                end if
            end do
        end do
    end subroutine broad_phase_sap_optimized

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
end module broad_phase_mod