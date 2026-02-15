from alkash3d.postproc.ssao import SSAOPass

def register(manager):
    """Регистрирует SSAO‑pass под именем 'ssao'."""
    manager.register_pass("ssao", SSAOPass)
