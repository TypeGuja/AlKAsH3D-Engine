# -*- coding: utf-8 -*-
import numpy as np
import pytest
from alkash3d.scene import Scene, Camera, Mesh, Node
from alkash3d.scene.light import DirectionalLight

def make_simple_mesh():
    verts = np.array([[0,0,0],[1,0,0],[0,1,0]], dtype=np.float32)
    inds = np.array([0,1,2], dtype=np.uint32)
    return Mesh(verts, indices=inds)

def test_scene_traversal():
    scene = Scene()
    cam = Camera()
    scene.add_child(cam)

    light = DirectionalLight()
    scene.add_child(light)

    cube = make_simple_mesh()
    scene.add_child(cube)

    names = [n.name for n in scene.traverse()]
    assert "Camera" in names
    assert "DirectionalLight" in names
    assert "Mesh" in names

def test_octree_build_and_query(scene):
    # `scene` уже построен в фикстуре ниже
    scene.culling.rebuild(scene)
    # Простейший frustum (None) = всё видимо
    visible = list(scene.visible_nodes(Camera()))
    # Должны видеть все узлы
    assert len(visible) >= 1
