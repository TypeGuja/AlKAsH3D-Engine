# -*- coding: utf-8 -*-
"""
alkash3d.utils.loader
--------------------

Utility helpers for loading external resources (binary files, text files,
JSON documents, images, etc.).  The module is intentionally lightweight
and does not depend on any part of the engine – only on the Python
standard library (and Pillow for image loading).

Historically the engine used a ``alkash3d.utils.loader`` module for
resource loading.  In the current code‑base most loaders have been split
into dedicated modules (e.g. ``texture_loader``), but a small compatibility
layer is still useful for user scripts and for backwards‑compatibility
with older examples.

The public API includes:
    * ``load_binary(path)`` – read raw bytes.
    * ``load_text(path, encoding='utf-8')`` – read a text file.
    * ``load_json(path)`` – parse a JSON file.
    * ``load_image(path)`` – load an image with Pillow and return a
      ``PIL.Image.Image`` instance.
    * ``load_texture(path)`` – alias for ``load_image`` (kept for
      compatibility with the old API).

All functions raise the standard Python exceptions (e.g. ``FileNotFoundError``,
``json.JSONDecodeError``) on error, which is suitable for the engine to
handle or let propagate.
"""

from __future__ import annotations

from pathlib import Path
import json
from typing import Any, Union

__all__ = [
    "load_binary",
    "load_text",
    "load_json",
    "load_image",
    "load_texture",
]


def _resolve_path(path: Union[str, Path]) -> Path:
    """
    Resolve ``path`` to an absolute :class:`~pathlib.Path` object.
    ``~`` expansion and relative paths are handled.
    """
    if isinstance(path, Path):
        p = path
    else:
        p = Path(path)
    return p.expanduser().resolve()


def load_binary(path: Union[str, Path]) -> bytes:
    """
    Load a file as raw bytes.

    Parameters
    ----------
    path : str or :class:`~pathlib.Path`
        Path to the file.

    Returns
    -------
    bytes
        The file contents.

    Raises
    ------
    FileNotFoundError
        If the file does not exist.
    """
    file_path = _resolve_path(path)
    return file_path.read_bytes()


def load_text(
    path: Union[str, Path],
    encoding: str = "utf-8",
) -> str:
    """
    Load a file as a Unicode string.

    Parameters
    ----------
    path : str or :class:`~pathlib.Path`
        Path to the file.
    encoding : str, optional
        Text encoding (default ``'utf-8'``).

    Returns
    -------
    str
        The decoded text.

    Raises
    ------
    FileNotFoundError
        If the file does not exist.
    UnicodeDecodeError
        If the file cannot be decoded with the given ``encoding``.
    """
    file_path = _resolve_path(path)
    return file_path.read_text(encoding=encoding)


def load_json(path: Union[str, Path]) -> Any:
    """
    Load a JSON file and return the parsed Python object.

    Parameters
    ----------
    path : str or :class:`~pathlib.Path`
        Path to the JSON file.

    Returns
    -------
    Any
        The result of :func:`json.load`, typically ``dict`` or ``list``.

    Raises
    ------
    FileNotFoundError
        If the file does not exist.
    json.JSONDecodeError
        If the file does not contain valid JSON.
    """
    file_path = _resolve_path(path)
    with file_path.open("r", encoding="utf-8") as f:
        return json.load(f)


def load_image(path: Union[str, Path]):
    """
    Load an image using Pillow.

    Parameters
    ----------
    path : str or :class:`~pathlib.Path`
        Path to the image file.

    Returns
    -------
    PIL.Image.Image
        The opened image.

    Raises
    ------
    FileNotFoundError
        If the image file does not exist.
    PIL.UnidentifiedImageError
        If Pillow cannot identify or open the file.
    """
    from PIL import Image  # Imported lazily to avoid hard dependency at import time

    file_path = _resolve_path(path)
    return Image.open(file_path)


# Backwards‑compatibility alias – older code used ``load_texture`` from this module.
load_texture = load_image