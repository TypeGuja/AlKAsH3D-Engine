import pathlib, os
print(pathlib.Path("alkash3d/resources/shaders").exists())        # True
print(list(pathlib.Path("alkash3d/resources/shaders").glob("*.glsl")))
