class Carrinho:
    def __init__(self, itens=[]):
        self.itens = itens

    def adicionar(self, item, opcoes={}):
        self.itens.append(item)
        opcoes["último"] = item
        return opcoes
