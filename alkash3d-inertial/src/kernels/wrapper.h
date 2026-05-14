// wrapper.h - заголовочный файл для генерации Rust биндингов

typedef struct {
    float position[3];
    float velocity[3];
    float acceleration[3];
    float angular_velocity[3];
    float mass;
    float inv_mass;
    float restitution;
    float friction;
    int is_static;
    int is_asleep;
} FortranRigidBody;

typedef struct {
    int body_a;
    int body_b;
    float normal[3];
    float penetration;
    float point[3];
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
void broad_phase_sap(const FortranRigidBody* bodies, int n, int* pairs, int* pair_count);
void broad_phase_sap_optimized(const FortranRigidBody* bodies, int n, int* pairs, int* pair_count);
void update_aabb(const FortranRigidBody* bodies, int n, float min_bounds[][3], float max_bounds[][3]);

// Narrow phase
int narrow_phase_gjk(const FortranRigidBody* body_a, const FortranRigidBody* body_b, FortranContact* contact);

// Solver
void integrate_bodies(FortranRigidBody* bodies, int n, float dt);
void solve_contacts(FortranRigidBody* bodies, FortranContact* contacts, int n_contacts, int iterations);
void solve_constraints(FortranRigidBody* bodies, FortranConstraint* constraints, int n_constraints, int iterations);