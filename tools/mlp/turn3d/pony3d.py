# Луна 3D: загрузка (как tools/pony.py), список мешей/материалов; рендеры ракурсов.
import json, math, os, sys
import bpy
from mathutils import Vector
ROOT = "G:/mlp_characters/mlp_3d"
PONIES = json.load(open(os.path.join(ROOT, "ponies.json"), encoding="utf-8"))
PREFER = ("closed", "normal", "regular")

def load(name):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.preferences.addon_enable(module="SourceIO")
    path = os.path.join(ROOT, PONIES[name])
    bpy.ops.sourceio.mdl(filepath=path, directory=os.path.dirname(path) + os.sep, files=[{"name": os.path.basename(path)}], write_qc=False)
    def walk(lc):
        for ch in lc.children:
            v = list(ch.children)
            if len(v) > 1 and all(not x.children for x in v):
                keep = next((x for p in PREFER for x in v if p in x.name.lower()), v[0])
                for x in v: x.exclude = x is not keep
            else: walk(ch)
    walk(bpy.context.view_layer.layer_collection)
    return [o for o in bpy.context.view_layer.objects if o.type == "MESH" and o.visible_get()]

argv = sys.argv[sys.argv.index("--") + 1:]
meshes = load(os.environ.get("PONY", "princess_luna"))
if argv[0] == "list":
    for o in meshes:
        print("MESH", o.name, len(o.data.vertices), [m.name for m in o.data.materials])
    for lc in bpy.context.view_layer.layer_collection.children:
        def pr(l, d=0):
            print("COLL", "  " * d + l.name, "EXCL" if l.exclude else "")
            for c in l.children: pr(c, d + 1)
        pr(lc)
if argv[0] == "groups":
    b = meshes[0]
    from collections import Counter
    cnt = Counter()
    for v in b.data.vertices:
        if v.groups:
            g = max(v.groups, key=lambda g: g.weight); cnt[b.vertex_groups[g.group].name] += 1
    for k, n in cnt.most_common(): print("VG", k, n)
# ---- карта частей: плоские цвета (Workbench), камера орто вокруг модели ----
PART = {  # категория → цвет
    "head": (1, 0, 0), "neck": (1, .55, 0), "torso": (.55, .55, 0),
    "fl_L": (0, .8, 0), "fl_R": (0, .4, 0), "hl_L": (0, .8, .8), "hl_R": (0, .4, .4),
    "mane": (0, 0, 1), "tail": (.5, 0, 1), "wing_L": (1, 0, 1), "wing_R": (.6, 0, .6),
    "horn": (1, 1, 0), "crown": (.9, .9, .5), "collar": (1, 1, 1), "shoes": (.5, .5, .5),
}
def bone_cat(n):
    side = "L" if n.startswith("Left") else "R" if n.startswith("Right") else ""
    m = n[len("Left"):] if side == "L" else n[len("Right"):] if side == "R" else n
    if m in ("Shoulder", "Forearm", "Hand", "Arm", "UpperArm", "Finger"): return "fl_" + side
    if m in ("Thigh", "Leg1", "Leg2", "Foot", "Ball", "Toe"): return "hl_" + side
    if n.startswith(("Chest", "Pelvis", "Spine", "Hips", "Root")): return "torso"
    if n == "Neck": return "neck"
    return "head"

def paint(objs):
    for o in objs:
        me = o.data
        attr = me.color_attributes.new("part", "FLOAT_COLOR", "POINT")
        nm = o.name.lower()
        fixed = next((k for k in ("mane", "tail", "horn", "crown", "collar", "shoes") if k in nm), None)
        if "wing" in nm: fixed = "wing_L" if "left" in nm else "wing_R"
        for v in me.vertices:
            if fixed: c = PART[fixed]
            elif v.groups:
                g = max(v.groups, key=lambda g: g.weight); c = PART.get(bone_cat(o.vertex_groups[g.group].name), (0, 0, 0))
            else: c = (0, 0, 0)
            attr.data[v.index].color = (*c, 1)
        me.color_attributes.active_color = attr
        # глаза/зубы — по материалу: в голову
        for i, m in enumerate(me.materials):
            if m and m.name in ("eyeball_r", "eyeball_l", "eyelashes", "eyebrow", "teeth", "tongue"):
                for p in me.polygons:
                    if p.material_index == i:
                        for vi in p.vertices: attr.data[vi].color = (*PART["head"], 1)

def rig_cam(objs, res=600):
    pts = [o.matrix_world @ Vector(c) for o in objs for c in o.bound_box]
    lo = Vector([min(p[i] for p in pts) for i in range(3)]); hi = Vector([max(p[i] for p in pts) for i in range(3)])
    ctr, size = (lo + hi) / 2, hi - lo
    scn = bpy.context.scene
    scn.render.resolution_x = scn.render.resolution_y = res
    scn.render.film_transparent = True
    scn.view_settings.view_transform = "Standard"
    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam")); cam.data.type = "ORTHO"
    cam.data.ortho_scale = max(size) * 1.15
    scn.collection.objects.link(cam); scn.camera = cam
    return cam, ctr, max(size) * 3

def place(cam, ctr, d, yaw):
    y = math.radians(yaw)
    cam.location = ctr + Vector((math.cos(y) * d, math.sin(y) * d, 0))
    cam.rotation_euler = (ctr - cam.location).to_track_quat("-Z", "Y").to_euler()

if argv[0] == "ids":
    out = argv[1]; os.makedirs(out, exist_ok=True)
    paint(meshes)
    scn = bpy.context.scene
    scn.render.engine = "BLENDER_WORKBENCH"
    sh = scn.display.shading; sh.light = "FLAT"; sh.color_type = "VERTEX"
    scn.render.filter_size = 0.0
    try: scn.display.render_aa = "OFF"
    except Exception: pass
    cam, ctr, d = rig_cam(meshes)
    step = float(argv[2]) if len(argv) > 2 else 5.625
    k = 0
    while k * step < 360:
        place(cam, ctr, d, k * step)
        scn.render.filepath = os.path.join(out, "id_%06.2f.png" % (k * step))
        bpy.ops.render.render(write_still=True); k += 1
    print("IDS DONE", k)

if argv[0] == "shade":
    out = argv[1]; os.makedirs(out, exist_ok=True)
    scn = bpy.context.scene
    engines = [e.identifier for e in bpy.types.RenderSettings.bl_rna.properties["engine"].enum_items]
    scn.render.engine = "BLENDER_EEVEE_NEXT" if "BLENDER_EEVEE_NEXT" in engines else "BLENDER_EEVEE"
    w = bpy.data.worlds.new("w"); w.use_nodes = True; w.node_tree.nodes["Background"].inputs[1].default_value = 1.0; scn.world = w
    cam, ctr, d = rig_cam(meshes)
    sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN")); sun.data.energy = 2.5
    scn.collection.objects.link(sun)
    for yaw in map(float, argv[2:]):
        place(cam, ctr, d, yaw)
        sun.rotation_euler = cam.rotation_euler
        scn.render.filepath = os.path.join(out, "sh_%06.2f.png" % yaw)
        bpy.ops.render.render(write_still=True)
    print("SHADE DONE")

if argv[0] == "marks":
    # Для DHX-углов 0..180 (шаг 5.625): по категориям частей — центр, рамка, глубина в координатах камеры (м; x вправо, y вверх).
    import bpy_extras
    out = argv[1]
    dg = bpy.context.evaluated_depsgraph_get()
    cam, ctr, d = rig_cam(meshes)
    pts = {}  # категория -> список мировых точек (вершины после деформации)
    for o in meshes:
        ev = o.evaluated_get(dg); me = ev.to_mesh()
        nm = o.name.lower()
        fixed = next((k for k in ("mane", "tail", "horn", "crown", "collar", "shoes") if k in nm), None)
        if "wing" in nm: fixed = "wing_L" if "left" in nm else "wing_R"
        for v in me.vertices:
            if fixed: c = fixed
            else:
                ov = o.data.vertices[v.index]
                if not ov.groups: continue
                g = max(ov.groups, key=lambda g: g.weight); c = bone_cat(o.vertex_groups[g.group].name)
            pts.setdefault(c, []).append(o.matrix_world @ v.co)
        ev.to_mesh_clear()
    import numpy as np
    P = {k: np.array([tuple(p) for p in v]) for k, v in pts.items()}
    res = {}
    k = 0
    while k <= 32:
        ang = k * 5.625
        place(cam, ctr, d, (270 + ang) % 360)
        bpy.context.view_layer.update()
        M = np.array(cam.matrix_world.inverted())
        r = {}
        for c, A in P.items():
            h = np.c_[A, np.ones(len(A))] @ M.T
            x, y, z = h[:, 0], h[:, 1], -h[:, 2]
            r[c] = {"cx": float(x.mean()), "cy": float(y.mean()), "x0": float(x.min()), "x1": float(x.max()),
                    "y0": float(y.min()), "y1": float(y.max()), "z": float(z.mean())}
        res["%.3f" % ang] = r; k += 1
    json.dump(res, open(out, "w"), indent=0)
    print("MARKS DONE", len(res), sorted(P))

if argv[0] == "bones":
    arm = next(o for o in bpy.data.objects if o.type == "ARMATURE")
    for b in arm.data.bones:
        print("BONE", b.name, "parent=" + (b.parent.name if b.parent else "-"), tuple(round(v, 3) for v in (arm.matrix_world @ b.head_local)), tuple(round(v, 3) for v in (arm.matrix_world @ b.tail_local)))

if argv[0] == "joints":
    # marks + суставы: для каждого угла DHX 0..180 — проекции голов костей (x, y, глубина) в координатах камеры.
    import numpy as np
    out = argv[1]
    arm = next(o for o in bpy.data.objects if o.type == "ARMATURE")
    J = ["Pelvis", "Chest2", "Neck", "Head", "LeftShoulder", "RightShoulder", "LeftBall", "RightBall", "LeftThigh", "RightThigh",
         "LeftFoot", "RightFoot", "tail_jiggle1", "tail_jiggle4", "tail_jiggle8", "hair_jiggle1", "hair_jiggle4", "hair_jiggle7",
         "LeftEarTip", "RightEarTip", "LeftWingClosed", "RightWingClosed", "LeftEyeLidUpper", "RightEyeLidUpper", "LeftEyeBrow", "RightEyeBrow", "Nose", "Jaw", "LeftLipUpMiddle", "LeftEar", "RightEar"]
    def alias(j):  # у разных моделей кости гривы/хвоста названы по-разному
        for c in (j, j.replace("hair_jiggle", "jiggle_hair"), j.replace("tail_jiggle", "jiggle_tail")):
            if c in arm.pose.bones: return c
    W = {j: arm.matrix_world @ arm.pose.bones[alias(j)].head for j in J if alias(j)}
    # копыто — низ: голова Ball/Foot ~ 0.1 над землёй, берём конец (tail) как подошву
    for j in ("LeftBall", "RightBall", "LeftFoot", "RightFoot"):
        W[j] = arm.matrix_world @ arm.pose.bones[j].tail
    cam, ctr, d = rig_cam(meshes)
    base = json.load(open(argv[2])) if len(argv) > 2 else {}
    for k in range(33):
        ang = k * 5.625
        place(cam, ctr, d, (270 + ang) % 360); bpy.context.view_layer.update()
        M = np.array(cam.matrix_world.inverted())
        r = base.setdefault("%.3f" % ang, {})
        r["J"] = {j: [float(v) for v in (M @ np.array([*p, 1]))[:3]] for j, p in W.items()}
        for j in r["J"]: r["J"][j][2] = -r["J"][j][2]
    json.dump(base, open(out, "w"))
    print("JOINTS DONE", len(W))
