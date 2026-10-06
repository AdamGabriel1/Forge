def bug(caminho):
    config = None
    if caminho == "prod":
        config = carregar("prod.yaml")
    config.load()   # ← FOR013: MaybeNone

def ok(caminho):
    config = None
    if caminho == "prod":
        config = carregar("prod.yaml")
    if config is not None:
        config.load()   # ← OK, refinado

def obvio():
    x = None
    x.foo()   # ← FOR013: DefinitelyNone
