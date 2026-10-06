import os

def bug():
    print(x)
    x = 1

def nao_bug():
    return 1
    print("morto")  # noqa: FOR011

print(os.getcwd())  # noqa
