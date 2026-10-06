# Forge

Linter estático e (futuro) formatter para Python, escrito em Rust.

O Forge analisa código Python com [tree-sitter](https://tree-sitter.github.io/tree-sitter/)
e reporta problemas de estilo, correção e possíveis bugs — sem executar o
código. A arquitetura em crates separa parsing, análise semântica, fluxo de
controle e regras, permitindo crescer cada camada de forma independente.

> ⚠️ **Projeto em desenvolvimento.** O formatter (`forge fmt`) ainda não está
> implementado. As 14 regras de lint e o autofix de 3 delas já funcionam.

---

## Instalação

```bash
git clone https://github.com/AdamGabriel1/Forge.git
cd Forge
cargo install --path crates/forge-cli
```

Isso instala o binário `forge` em `~/.cargo/bin/forge`. Se o comando não
aparecer, adicione `~/.cargo/bin` ao `PATH`.

Durante o desenvolvimento, use `cargo run -- <args>` em vez de instalar.

---

## Uso

### `forge check`

Roda o linter em arquivos ou diretórios.

```bash
forge check src/
forge check main.py
forge check . --strict                 # warnings contam como erro
forge check . --format json            # saída em JSON
forge check . --baseline forge-baseline.json
```

**Exit codes:**
- `0` — nenhum problema (ou só warnings sem `--strict`)
- `1` — erros encontrados (ou warnings com `--strict`)
- `2` — erro de configuração

### `forge fix`

Aplica correções automáticas para as regras que oferecem autofix.

```bash
forge fix src/                # aplica e escreve no disco
forge fix src/ --dry-run      # mostra o resultado sem escrever
forge fix src/ --check        # sai com 1 se há correções pendentes (útil em CI)
```

Regras com autofix hoje: **FOR001** (`except:` → `except Exception:`),
**FOR006** (imports não usados), **FOR011** (código inalcançável).

### `forge baseline`

Gera um baseline que suprime os diagnósticos **existentes** nas próximas
execuções. Útil para adotar o Forge em projetos legados sem precisar
limpar tudo de uma vez.

```bash
forge baseline src/ --output forge-baseline.json
forge check src/ --baseline forge-baseline.json
```

A chave de supressão é `(código, arquivo, conteúdo da linha)`. Se você
**editar a linha**, o diagnóstico volta a aparecer — o baseline só
silencia o problema enquanto o código permanecer idêntico.

### `forge explain`

Explica uma regra, ou lista todas.

```bash
forge explain FOR001
forge explain --list
```

### Supressão inline com `# noqa`

```python
import os  # noqa: FOR006
try:
    ...
except:  # noqa
    ...
```

- `# noqa` — suprime tudo na linha
- `# noqa: FOR001` — suprime só a regra
- `# noqa: FOR001, FOR006` — suprime várias

---

## Regras

| Código  | Nome                              | Descrição                                                      |
|---------|-----------------------------------|----------------------------------------------------------------|
| FOR001  | `bare_except`                     | `except:` sem exceção captura `SystemExit` e `KeyboardInterrupt`. |
| FOR002  | `mutable_default_argument`        | `def f(x=[])` — default mutável é compartilhado entre chamadas. |
| FOR003  | `too_many_arguments`              | Função com muitos parâmetros é difícil de testar e evoluir.    |
| FOR004  | `function_too_long`               | Funções muito longas dificultam a leitura.                    |
| FOR005  | `shadowed_builtin`                | `list = []` sobrescreve um builtin.                            |
| FOR006  | `unused_import`                   | Import que nunca é usado.                                     |
| FOR007  | `unused_variable`                 | Variável atribuída mas nunca lida.                            |
| FOR008  | `shadowed_variable`               | Variável local com mesmo nome de uma externa.                 |
| FOR009  | `redefined_function`              | Duas `def` com o mesmo nome no mesmo escopo.                  |
| FOR010  | `undefined_name`                  | Nome usado mas nunca definido, importado ou builtin.          |
| FOR011  | `unreachable_code`                | Código depois de `return`/`raise`/`break`/`continue`.          |
| FOR012  | `used_before_assignment`          | Variável local lida antes de ser atribuída.                   |
| FOR013  | `possible_none_dereference`       | `x = None; x.foo()` sem checagem intermediária.               |
| FOR014  | `expensive_operation_inside_loop` | `sorted()`, `reversed()`, `.sort()`, comprehensions dentro de loops. |

---

## Configuração

O Forge procura por um `forge.toml` a partir do diretório analisado, subindo
até a raiz. Um arquivo típico:

```toml
[lint]
# Se `select` estiver vazio, todas as regras são habilitadas.
select = ["FOR001", "FOR002", "FOR006", "FOR010"]

# `ignore` tem precedência sobre `select`.
ignore = ["FOR004"]

# Override de severidade por regra.
severity = { FOR001 = "error" }

# Opções específicas por regra.
[lint.options.FOR003]
max-args = 8

[lint.options.FOR004]
max-lines = 80
```

---

## Arquitetura

Workspace Rust com 6 crates:

```
crates/
├── forge-core       # Diagnostic, Severity, Range, Edit, Config, noqa, baseline
├── forge-parser     # wrapper sobre tree-sitter-python
├── forge-semantic   # escopos, bindings, uses, resolução de nomes
├── forge-cfg        # terminadores de fluxo, detecção de código inalcançável
├── forge-lint       # trait Rule, RuleRegistry, as 14 regras
└── forge-cli        # binário `forge` (check, fix, baseline, explain)
```

Cada regra implementa a trait `Rule` e é registrada em `default_registry()`.
Adicionar uma regra nova é uma struct + uma linha de registro.

---

## Desenvolvimento

```bash
# Build
cargo build

# Testes (161 em forge-lint + 4 em forge-core)
cargo test

# Só um crate
cargo test -p forge-lint

# Ver o registry
cargo run -- explain --list
```

Antes de abrir PR, rode `cargo fmt` e `cargo clippy`.

---

## Limitações conhecidas

- **FOR013** é linear por escopo: não modela branches de `if`. Reporta
  falsos positivos em `x = 5; if c: x = None; x.foo()` e falsos negativos
  em `x = None; if c: x = 5; x.foo()`. Quando tivermos um CFG com merge de
  estados, migramos a regra para data-flow real.
- **FOR014** só reconhece `sorted`, `reversed`, `.sort()` e comprehensions.
  Chamadas potencialmente caras a métodos customizados ficam de fora.
- **`forge fmt`** ainda não existe. O subcomando está stubado.
- **`forge fix`** cobre só 3 das 14 regras; as demais não oferecem autofix.

---

## Licença

MIT.
