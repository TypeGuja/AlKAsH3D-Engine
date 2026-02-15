import pathlib, os
print(pathlib.Path("../resources/shaders").exists())        # True
print(list(pathlib.Path("../resources/shaders").glob("*.hlsl")))
