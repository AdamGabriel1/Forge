# Forge

[![CI](https://github.com/AdamGabriel1/Forge/actions/workflows/ci.yml/badge.svg)](https://github.com/AdamGabriel1/Forge/actions/workflows/ci.yml)

Linter, formatter e ferramenta de adoção incremental para Python, escrito em
Rust sobre [tree-sitter](https://tree-sitter.github.io/tree-sitter/).

O Forge combina três coisas que normalmente vivem em ferramentas separadas:

- **Lint** com 16 regras, incluindo análise de fluxo sensível a caminho
  (nullable, definite assignment, constant propagation, reaching
  definitions) com iteração até ponto fixo em loops.
- **Formatter** opinativo, idempotente, preservando strings e comentários.
- **Baseline** para adoção incremental em projetos legados: você gera um
  snapshot dos problemas existentes e passa a enxergar só o que é novo.

> ⚠️ **Projeto em desenvolvimento.** O formatter cobre espaçamento,
> indentação e continuations, mas ainda não quebra linhas longas nem
> reformata docstrings.

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
forge check . --strict                       # warnings contam como erro
forge check . --format json                  # saída em JSON
forge check . --baseline forge-baseline.json
forge check . --summary by-rule              # agrupa por código de regra
forge check . --summary by-file              # agrupa por arquivo
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

Regras com autofix hoje:

| Regra  | Correção                                                        |
|--------|-----------------------------------------------------------------|
| FOR001 | `except:` → `except Exception:`                                 |
| FOR002 | `def f(x=[])` → `def f(x=None): if x is None: x = []`           |
| FOR006 | Remove imports não usados                                       |
| FOR011 | Remove código inalcançável após `return`/`raise`/`break`/`continue` |

### `forge fmt`

Formata código Python. Idempotente, preserva strings e comentários.

```bash
forge fmt src/                # escreve no disco
forge fmt src/ --check        # sai com 1 se algum arquivo precisa
forge fmt src/ --diff         # mostra diff unificado, não escreve
```

Regras implementadas:

- **Espaçamento**: vírgulas, operadores binários, `and`/`or`/`not`,
  `is`/`in`, unários, `=` e `+=`, `:=`, `->`
- **Delimitadores**: sem espaço dentro de `()`/`[]`/`{}`, sem espaço
  antes de `.`/`(`/`[`, sem espaço antes de `:` de cabeçalho de bloco
- **Colons contextuais**: `{1: 2}` (dict), `def f(x: int)` (anotação),
  `a[1:2]` (slice sem espaço)
- **Indentação de bloco**: normaliza para 4 espaços por nível
- **Continuations multi-linha**: indentação dentro de `()`/`[]`/`{}`
  normalizada para `base + 4 * profundidade`, fechadores alinhados
- **Decoradores**: `@app.route` colado ao nome

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

| Código  | Nome                              | Descrição                                                       |
|---------|-----------------------------------|-----------------------------------------------------------------|
| FOR001  | `bare_except`                     | `except:` sem exceção captura `SystemExit` e `KeyboardInterrupt`. |
| FOR002  | `mutable_default_argument`        | `def f(x=[])` — default mutável é compartilhado entre chamadas. |
| FOR003  | `too_many_arguments`              | Função com muitos parâmetros é difícil de testar e evoluir.     |
| FOR004  | `function_too_long`               | Funções muito longas dificultam a leitura.                      |
| FOR005  | `shadowed_builtin`                | `list = []` sobrescreve um builtin.                             |
| FOR006  | `unused_import`                   | Import que nunca é usado.                                       |
| FOR007  | `unused_variable`                 | Variável atribuída mas nunca lida.                              |
| FOR008  | `shadowed_variable`               | Variável local com mesmo nome de uma externa.                   |
| FOR009  | `redefined_function`              | Duas `def` com o mesmo nome no mesmo escopo.                    |
| FOR010  | `undefined_name`                  | Nome usado mas nunca definido, importado ou builtin.            |
| FOR011  | `unreachable_code`                | Código depois de `return`/`raise`/`break`/`continue`.           |
| FOR012  | `used_before_assignment`          | Variável local pode ser lida antes de receber valor.            |
| FOR013  | `possible_none_dereference`       | `x = None; x.foo()` sem checagem intermediária.                 |
| FOR014  | `expensive_operation_inside_loop` | `sorted()`, `.sort()`, comprehensions dentro de loops.          |
| FOR015  | `constant_condition`              | Condição de `if`/`while` sempre avalia para `True`/`False`.     |
| FOR016  | `dead_store`                      | Atribuição cujo valor nunca é lido.                             |

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

Workspace Rust com 7 crates:

```
crates/
├── forge-core       # Diagnostic, Severity, Range, Edit, Config, noqa, baseline
├── forge-parser     # wrapper sobre tree-sitter-python
├── forge-semantic   # escopos, bindings, uses, resolução de nomes
├── forge-cfg        # data-flow engine: Analysis trait + 4 análises
├── forge-format     # formatter (gaps, indentação, continuations)
├── forge-lint       # trait Rule, RuleRegistry, 16 regras
└── forge-cli        # binário `forge` (check, fix, fmt, baseline, explain)
```

### Motor de data-flow

`forge-cfg` expõe um trait `Analysis` que permite implementar análises
sensíveis a caminho sem materializar um CFG explícito:

```rust
pub trait Analysis {
    type State: Clone + PartialEq;
    fn initial(&self) -> Self::State;
    fn transfer(&self, node: Node, state: &State, diags: &mut Vec<Diagnostic>) -> State;
    fn refine(&self, cond: Node, state: &State, positive: bool) -> State;
    fn merge(&self, a: &State, b: &State) -> State;
    fn bind_loop_target(&self, target: Node, state: &State) -> State { ... }
    fn observe_condition(&self, cond: Node, state: &State, diags: &mut Vec<Diagnostic>) { }
}
```

O walker recursivo (`run_block`) faz:

- **`if/else`**: dois ramos independentes que fazem merge no ponto de
  junção, com `refine` aplicado à condição em cada lado.
- **`while`/`for`**: iteração até **ponto fixo**. O estado no loop head
  é recomputado até `new_head == loop_head` (com teto de 64 iterações
  como garantia de terminação). Diagnósticos são emitidos apenas da
  última iteração, evitando duplicação.

Quatro análises já implementadas:

- **`NullableAnalysis`** — rastreia se uma variável pode ser `None` (FOR013).
- **`DefiniteAssignmentAnalysis`** — rastreia se uma variável foi atribuída
  em todos os caminhos até um ponto (FOR012).
- **`ConstantPropagation`** — propaga constantes (`int`, `str`, `bool`,
  `None`) por expressões aritméticas, comparações e booleanos (FOR015).
- **`ReachingDefinitions`** — rastreia definições ativas e detecta dead
  stores, sem duplicar com FOR007 (FOR016).

---

## Desenvolvimento

```bash
# Build
cargo build

# Testes (296 no total)
cargo test --all

# Só um crate
cargo test -p forge-lint
cargo test -p forge-format
cargo test -p forge-cfg

# Ver o registry
cargo run -- explain --list
```

Antes de abrir PR: `cargo fmt --all` e
`cargo clippy --all-targets --all-features -- -D warnings`. O CI roda
exatamente esses comandos mais `cargo test --all`.

---

## Performance

Medido em Codespaces (2 vCPU), 200 arquivos × 1100 linhas = 220k linhas:

| Modo                          | Tempo  |
|-------------------------------|--------|
| Serial (`RAYON_NUM_THREADS=1`) | ~4.4s  |
| Paralelo (default)            | ~3.1s  |

`check`, `fix`, `baseline` e `fmt` rodam em paralelo por arquivo via
`rayon`. Cada thread mantém seu próprio `tree-sitter::Parser` (não é
`Sync`).

---

## Limitações conhecidas

- **FOR012**/**FOR013** aproximam loops com ponto fixo, mas o teto de 64
  iterações pode truncar análises teóricas mais longas. Na prática,
  convergência acontece em poucas iterações para as duas análises atuais.
- **FOR014** só reconhece `sorted`, `reversed`, `.sort()` e comprehensions.
  Chamadas potencialmente caras a métodos customizados ficam de fora.
- **FOR015** não propaga constantes através de argumentos de função nem
  de atributos (`self.DEBUG = False`).
- **FOR016** reporta qualquer atribuição cujo valor não foi lido, mas
  não distingue “dead store garantida” de “dead store em algum caminho”
  — o output é por definição individual.
- **`forge fmt`** não quebra linhas longas nem reformata docstrings.
  Continuations de backslash (`x = 1 + \`) são preservadas.
- **`forge fix`** cobre 4 das 16 regras.

---

## Licença

MIT.