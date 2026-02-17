from setuptools import setup, find_packages

setup(
    name="alkash3d",
    version="0.1.0",
    description="AlKAsH3D Game Engine",
    author="Your Name",
    packages=find_packages(),
    install_requires=[
        "numpy>=1.20.0",  # Исправлено: убраны лишние символы
        "glfw>=1.12.0",
        "Pillow>=8.0.0",
        "PyGLM>=2.0.0",
    ],
    python_requires=">=3.8",
)