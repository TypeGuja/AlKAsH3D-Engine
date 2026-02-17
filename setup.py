from setuptools import setup, find_packages

setup(
    name="alkash3d",
    version="0.1.0",
    description="AlKAsH3D Game Engine",
    author="Your Name",
    packages=find_packages(),
    install_requires=[
        "numpy>=1.20.0",
        "glfw>=1.12.0",
        "Pillow>=8.0.0",
        "PyGLM>=2.0.0",
    ],
    python_requires=">=3.8",
    classifiers=[
        "Development Status :: 3 - Alpha",
        "Intended Audience :: Developers",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.8",
        "Programming Language :: Python :: 3.9",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
        "Programming Language :: Python :: 3.12",
    ],
)