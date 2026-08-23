// wrapper.h - заголовочный файл для справки / генерации биндингов
//
// ИСПРАВЛЕНО: раньше этот файл был устаревшим — FortranRigidBody тут был
// МЕНЬШЕ реальной структуры (не было inertia/inv_inertia/
// angular_acceleration/linear_damping/angular_damping), то есть не
// совпадал с актуальным rigid_body.f90/ffi/mod.rs. Теперь синхронизирован
// побайтово с обеими сторонами.

typedef struct {
    float position[3];
    float velocity[3];
    float acceleration[3];
    float angular_velocity[3];
    float angular_acceleration[3];
    float inertia[3][3];
    float inv_inertia[3][3];
    float mass;
    float inv_mass;
    float restitution;
    float friction;
    float linear_damping;
    float angular_damping;
    int is_static;
    int is_asleep;
} FortranRigidBody;

typedef struct {
    int body_a;
    int body_b;
    float normal[3];
    float penetration;
    float point[3];
    float tangent1[3];
    float tangent2[3];
    float friction_impulse[2];
} FortranContact;

typedef struct {
    int body_a;
    int body_b;
    float anchor_a[3];
    float anchor_b[3];
    float bias;
    float accumulated_impulse;
} FortranConstraint;

// Broad phase
// ИСПРАВЛЕНО: добавлен max_pairs — реальная ёмкость cell_pairs (в парах),
// без которого запись могла уйти за пределы буфера (см. broad_phase.f90).
void broad_phase_grid(const FortranRigidBody* bodies, int n, float cell_size,
                       int grid_width, int grid_height,
                       int* cell_starts, int* cell_counts,
                       int* cell_pairs, int* pair_count, int max_pairs);
void broad_phase_sap_optimized(const FortranRigidBody* bodies, int n,
                                const int* active_indices, int active_count,
                                int* pairs, int* pair_count);
void broad_phase_temporal(const FortranRigidBody* bodies, int n,
                           const int* active_list, int active_count,
                           int* pairs, int* pair_count,
                           float radius, float time_step);

// Narrow phase
int narrow_phase_gjk(const FortranRigidBody* body_a, const FortranRigidBody* body_b,
                      FortranContact* contact);
void generate_collision_pairs(const FortranRigidBody* bodies, int n,
                               int* pairs, int* pair_count, float radius);

// Solver
void integrate_bodies(FortranRigidBody* bodies, int n, float dt);
void batch_integrate(FortranRigidBody* bodies, int n, float dt, float gravity,
                      int start_idx, int end_idx);
void solve_contacts(FortranRigidBody* bodies, FortranContact* contacts,
                     int n_contacts, int iterations);
void solve_contacts_vectorized(FortranRigidBody* bodies, FortranContact* contacts,
                                int n_contacts, int iterations, float dt);
void solve_constraints(FortranRigidBody* bodies, FortranConstraint* constraints,
                        int n_constraints, int iterations);
void resolve_penetration_batch(FortranRigidBody* bodies, FortranContact* contacts,
                                int n_contacts);

// AABB и вспомогательные функции
void update_aabb(const FortranRigidBody* bodies, int n,
                  float* min_bounds, float* max_bounds, float radius);
void update_aabb_vectorized(const FortranRigidBody* bodies, int n,
                             float* min_bounds, float* max_bounds, float radius);
// ИСПРАВЛЕНО: добавлены sleep_time/sleep_timers — без них не было
// персистентного состояния между кадрами для гистерезиса сна.
void update_sleep_state(FortranRigidBody* bodies, int n, float dt,
                         float sleep_threshold, float sleep_time,
                         float* sleep_timers);
void compute_center_of_mass(const FortranRigidBody* bodies, int n,
                             float* center, float* total_mass);
