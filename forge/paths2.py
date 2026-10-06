def bug(caminho):
    if caminho == "prod":
        config = carregar("prod.yaml")
    # Não há else — config pode não existir aqui
    print(config)   # ← FOR012

def ok(caminho):
    if caminho == "prod":
        config = carregar("prod.yaml")
    else:
        config = carregar("default.yaml")
    print(config)   # ← OK

def com_default(caminho):
    config = None
    if caminho == "prod":
        config = carregar("prod.yaml")
    print(config)   # ← OK
