# setup.py
from setuptools import setup, find_packages

setup(
    name="alkash3d",
    version="1.0.0",
    description="AlKAsH3D Game Engine",
    packages=find_packages(),
    install_requires=[
        "numpy>=1.20.0",
        "glfw>=2.5.0",
        "PyOpenGL>=3.1.5",
        "Pillow>=9.0.0",
        "numba>=0.55.0",
    ],
    package_data={
        'alkash3d': ['resources/shaders/*.glsl'],
    },
)
