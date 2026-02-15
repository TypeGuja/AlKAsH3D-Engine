# editor_app/main.py
"""
Запуск редактора в виде отдельного приложения.

    python -m editor_app            # через __main__
    python editor_app/main.py        # напрямую
"""

import sys
from PySide6.QtWidgets import QApplication

from editor_app.ui import MainWindow


def run():
    """Создаёт Qt‑приложение и открывает главное окно."""
    app = QApplication(sys.argv)

    # Устанавливаем стиль, похожий на Unity
    app.setStyle('Fusion')

    win = MainWindow()
    win.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    run()