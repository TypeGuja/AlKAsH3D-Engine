# alkas3d/multithread/task_pool.py
# ---------------------------------------------------------------
# Простой пул задач на основе concurrent.futures.
# Позволяет распределять тяжёлые CPU‑операции (culling, animation)
# по нескольким ядрам.  Для GPU‑загрузок используем CUDA‑очереди.
# ---------------------------------------------------------------

from concurrent.futures import ThreadPoolExecutor, as_completed
import queue
import threading

class TaskPool:
    """Пул готового количества потоков; задачи принимаются как callables."""
    def __init__(self, max_workers=None):
        self.executor = ThreadPoolExecutor(max_workers=max_workers)
        self.tasks = queue.Queue()
        self._shutdown = False

    def submit(self, fn, *args, **kwargs):
        """Отправить задачу в пул, вернуть Future."""
        if self._shutdown:
            raise RuntimeError("TaskPool already shut down")
        future = self.executor.submit(fn, *args, **kwargs)
        self.tasks.put(future)
        return future

    def wait_all(self):
        """Блокировать до завершения всех поставленных задач."""
        while not self.tasks.empty():
            future = self.tasks.get()
            future.result()  # пробрасывает исключения, если они возникли

    def shutdown(self, wait=True):
        self._shutdown = True
        self.executor.shutdown(wait=wait)
