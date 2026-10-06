def f():
    config = None
    config.load()  # FOR013

def g():
    config = None
    config = {"a": 1}
    config.load()  # OK
