import sys

def perigo():
    try:
        faz_algo_arriscado()
    except:
        print("Capturou tudo, incluindo Ctrl+C!")

def ok():
    try:
        faz_algo_arriscado()
    except Exception as e:
        print(f"Capturou só exceções: {e}")
