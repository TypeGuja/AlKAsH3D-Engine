# -*- coding: utf-8 -*-
"""
Минимальный парсер Wavefront OBJ (только позиции, нормали, texcoords, индексы).
Не поддерживает материалы MTL – для примера достаточно.
"""
import numpy as np

def load_obj(path):
    verts = []
    normals = []
    texcoords = []
    faces = []   # (pos, tex, norm) triples (1‑based)

    with open(path, 'r', encoding='utf-8') as f:
        for line in f:
            if line.startswith('#') or not line.strip():
                continue
            parts = line.split()
            if parts[0] == 'v':
                verts.append(list(map(float, parts[1:4])))
            elif parts[0] == 'vn':
                normals.append(list(map(float, parts[1:4])))
            elif parts[0] == 'vt':
                texcoords.append(list(map(float, parts[1:3])))
            elif parts[0] == 'f':
                face = []
                for v in parts[1:]:
                    # форматы: v, v/vt, v//vn, v/vt/vn
                    idx = v.split('/')
                    p = int(idx[0]) - 1
                    t = int(idx[1]) - 1 if len(idx) > 1 and idx[1] else -1
                    n = int(idx[2]) - 1 if len(idx) > 2 and idx[2] else -1
                    face.append((p, t, n))
                faces.append(face)

    # Превратим в плоские массивы и сформируем индексы
    vertex_data = []
    index_data = []
    vert_dict = {}   # map (p,t,n) -> index
    for face in faces:
        # OBJ обычно хранит полигоны, часто треугольники; здесь делаем треугольник fan
        if len(face) < 3:
            continue
        v0 = face[0]
        for i in range(1, len(face)-1):
            tri = [v0, face[i], face[i+1]]
            for p, t, n in tri:
                key = (p, t, n)
                if key not in vert_dict:
                    # позиция
                    vertex = verts[p]
                    # нормаль (может быть -1)
                    normal = normals[n] if n >= 0 else [0.0, 0.0, 1.0]
                    # texcoord
                    tex = texcoords[t] if t >= 0 else [0.0, 0.0]

                    vertex_data.extend(vertex + normal + tex)
                    vert_dict[key] = len(vert_dict)
                index_data.append(vert_dict[key])

    # Приведём к numpy массивам
    vertex_arr = np.array(vertex_data, dtype=np.float32).reshape(-1, 8)  # 3+3+2
    positions = vertex_arr[:, 0:3]
    normals = vertex_arr[:, 3:6]
    texcoords = vertex_arr[:, 6:8]
    indices = np.array(index_data, dtype=np.uint32)

    return positions, normals, texcoords, indices
