# Detecção Apple Silicon: chip real e paridade com o simulador

## Validação no Mac real — 8 de novembro de 2026

Tarefa: [#70](https://github.com/MLX-Pilot/MLX-Pilot/issues/70).
O patch recebido no ZIP foi aplicado sem conflitos com `git am`, após
`git pull --ff-only` da main até `cb21a55`. O commit de origem `c2440f5`
foi reaplicado como `ec966b7` na branch `fix/apple-silicon-chip-detection`.
A publicação desta branch é separada do merge, que continua dependendo de revisão.

Ambiente desta validação: Mac mini M4, macOS 26.3.1 (25D2128), Rust 1.93.0,
24 GiB de memória unificada e dez núcleos. As consultas reais retornaram:

```text
/usr/sbin/sysctl -n machdep.cpu.brand_string → Apple M4
/usr/sbin/sysctl -n hw.memsize → 25769803776
uname -m → arm64
/usr/bin/arch -x86_64 /usr/sbin/sysctl -n sysctl.proc_translated → 1
/usr/bin/arch -x86_64 /usr/sbin/sysctl -n machdep.cpu.brand_string → Apple M4
```

A detecção real, tanto no binário ARM64 quanto no binário x86_64 executado
sob Rosetta, produziu `Apple M4`, dez núcleos, 24 GiB de RAM, uma GPU
`Apple M4 GPU`, backend Metal, 18 GiB estimados para GPU e **120 GB/s**.
O teste específico compara também rankings e perfis de execução com o simulador.

| Verificação desta execução | Resultado | Evidência |
| --- | --- | --- |
| `cargo test --locked --workspace` | 493 passaram, zero falhas; um teste de hardware ignorado por padrão | [log](evidence/apple-silicon-2026-11-08/workspace-tests.log) |
| Teste ignorado `real_m4_24gb_matches_simulator`, ARM64 nativo | Passou no Mac real | [log](evidence/apple-silicon-2026-11-08/real-m4-arm64.log) |
| Bibliotecas hardware-fit/model-fit, `--target x86_64-apple-darwin` | 36 passaram sob Rosetta; um ignorado | [log](evidence/apple-silicon-2026-11-08/rosetta-tests.log) |
| Mesmo teste específico do M4/24, binário x86_64 | Passou sob Rosetta | [log](evidence/apple-silicon-2026-11-08/real-m4-rosetta.log) |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | Sem avisos | [log](evidence/apple-silicon-2026-11-08/clippy.log) |
| `cargo fmt --all -- --check`, `git diff --check` | Passaram | Executados localmente |
| `cargo build --locked -p mlx-ollama-daemon` | Passou | Daemon usado no smoke abaixo |
| `node --test apps/desktop-ui/e2e/hardware-mac-simulator.test.js` | Quatro passaram | [log](evidence/apple-silicon-2026-11-08/ui-tests.log) |
| API do daemon ARM64 corrigido | HTTP 200; 60 resultados de modelos e dois conjuntos de perfis idênticos entre detectado/simulado; POST de simulação também equivalente | [resumo](evidence/apple-silicon-2026-11-08/api-summary.txt), [hardware real](evidence/apple-silicon-2026-11-08/system-arm64.json) |
| Navegador Chromium com daemon real | Escanear, simular Mac mini M4/24 e restaurar hardware real; 15 linhas de ranking idênticas; zero erros no console após daemon pronto | [paridade](evidence/apple-silicon-2026-11-08/ui-parity.txt), [console](evidence/apple-silicon-2026-11-08/ui-console.txt) |

O smoke usou estado temporário, UI em `127.0.0.1:5597` e daemon em
`127.0.0.1:11437`, sem alterar a configuração persistente do aplicativo.
Na primeira abertura houve conexões recusadas antes de o daemon ficar pronto;
a conferência foi repetida após a inicialização. O selo global “Degradado” nas
capturas decorre do autostart do Ollama desativado nesse ambiente isolado;
as rotas de hardware responderam normalmente. A validação visual cobre a UI
web do desktop em Chromium; o aplicativo empacotado Tauri não foi executado.

[Hardware detectado](evidence/apple-silicon-2026-11-08/apple-m4-detected.png) ·
[Simulação Mac mini M4/24](evidence/apple-silicon-2026-11-08/apple-m4-simulated.png)

### Impacto reproduzido no motor de estimativas ARM64

O teste `m4_24gb_bandwidth_impact_on_default_catalog` isolou a mudança de
150 para 120 GB/s com os mesmos 15 modelos do catálogo padrão. Mudaram de
posição seis modelos em `general` e seis em `coding`; nenhum em `creative`
ou `analysis`. Os 60 pares mantiveram quantização, memória estimada e fit.
O simulador manual já usava 120 GB/s e continua produzindo os mesmos resultados;
a correção alinha a detecção real a ele.

| Modelo, uso geral | TPS antes → corrigido | Posição antes → corrigida |
| --- | --- | --- |
| Llama 3 8B Instruct, Q8_0 | 24,375 → 19,500 | 2 → 6 |
| Gemma 2 9B IT, Q8_0 | 21,195652 → 16,956522 | 9 → 9 |
| Llama 3 70B Instruct, Q2_K | 13,508339 → 11,897027 | 14 → 14 |

[Saída completa do experimento ARM64](evidence/apple-silicon-2026-11-08/catalog-impact-arm64.log).
Estes números são estimativas do software executado no Mac, **não um benchmark
de inferência física**. O offload parcial difere dos valores Linux históricos
abaixo porque a heurística de banda da CPU depende da arquitetura compilada.

### Limitações atuais

- Os caminhos de consulta real e o teste de paridade foram executados em ARM64
  e sob Rosetta. O daemon completo e a UI foram exercitados em ARM64; não houve
  smoke HTTP/visual do daemon completo x86_64 nem teste em um Mac Intel físico.
- Falhas de sysctl, nomes desconhecidos e ausência de evidência ARM continuam
  cobertos por respostas injetadas nos testes; não se provocaram falhas no SO.
- 120 GB/s é uma especificação cadastrada, não uma medição. A parcela de 75%
  para GPU e o fallback desconhecido de 150 GB/s continuam heurísticas.
- A tabela preexistente de outros chips não foi auditada; configurações em que
  a memória não distingue o bin ainda usam o bin superior.
- Não houve execução do app Tauri empacotado nem benchmark de inferência.

## Registro histórico da execução original Linux

O texto abaixo veio do ZIP e descreve exclusivamente a execução anterior em
Linux. As pendências de detecção nativa, Rosetta e UI web mencionadas nesse
registro foram verificadas nesta execução conforme os resultados acima.

Base: `cb21a55b0d5213983fd62ee3cfc21d405b84daa0` (PR #69). Caso principal:
Mac mini com Apple M4 e 24 GB de memória unificada. Os perfis usados nos testes
Linux são **sintéticos**, não capturas do computador do usuário.

### Problema e correção

`hw.model` retorna o identificador do computador, como `Mac16,10`, não o chip.
Consultá-lo na tabela de chips levava ao fallback de 150 GB/s, enquanto o
simulador do M4 usava 120 GB/s. O teste antigo de presença de `Mac` no nome
também podia classificar um Mac Intel como Apple Silicon.

A detecção agora consulta `/usr/sbin/sysctl -n machdep.cpu.brand_string`, com
limite de dois segundos por consulta, e usa o nome de CPU do `sysinfo` como
alternativa. O XNU expõe nessa chave o `product-soc-name` do device tree.
Um nome Apple M reconhecível é evidência do chip, inclusive para gerações
futuras sem especificações cadastradas. Se o nome não estiver disponível,
a arquitetura ARM nativa, `hw.optional.arm64` ou `sysctl.proc_translated`
permitem manter o perfil Metal genérico. Sem evidência de host ARM, a detecção
Apple é recusada e o fluxo normal de detecção continua.

A memória total já obtida pelo `sysinfo` é usada tanto no perfil quanto na
consulta à tabela, incluindo variantes dependentes da capacidade instalada.
`apple_gpu_info` monta a GPU nos caminhos detectado e simulado. O nome da CPU
passa a refletir o chip resolvido; os núcleos da CPU real continuam vindo do SO.
A consulta anterior a `hw.perflevel1.gpu_count` foi removida: ela não é necessária
para resolver a banda e o nome da GPU agora identifica o chip.

Chips desconhecidos preservam o fallback de 150 GB/s. Esse número é uma
**heurística de compatibilidade**, não uma medição nem um limite inferior
garantido. Não é inferida uma geração pelo identificador do Mac. Memória
inválida não gera GPU de memória zero/NaN.

| Campo | M4 detectado antes | M4 detectado corrigido | M4 simulado |
| --- | ---: | ---: | ---: |
| Memória total | 24 GiB | 24 GiB | 24 GiB |
| Memória estimada para GPU (75%) | 18 GiB | 18 GiB | 18 GiB |
| Banda | 150 GB/s | 120 GB/s | 120 GB/s |
| Backend | Metal | Metal | Metal |

A capacidade é calculada em GiB, mantendo a convenção anterior da interface
que a rotula como GB; a banda segue GB/s decimal das especificações Apple.

### Regressões e execução

- Resolução de consultas do SO injetada nos testes, exercitando o mesmo caminho
  usado em produção: M4/24, M1, M2 Pro, M3 Max e M4 Pro/Max; bins por memória;
  falha/saída vazia da consulta; gerações futuras; host Intel; Rosetta; ausência
  de evidência ARM; e memória inválida.
- Comparação explícita dos campos de hardware detectados com os simulados.
- Paridade de ranking e perfis de execução para 8B e 70B, quatro usos,
  quantização automática/Q4_K_M e contextos de 4096/16384.
- Comparação isolando apenas a banda 150 → 120 GB/s no catálogo padrão real
  do daemon, nos quatro usos. O teste imprime posições, TPS, scores e quants.

```sh
cargo fmt --all -- --check
cargo test --locked -p mlx-hardware-fit -p mlx-model-fit -p mlx-ollama-daemon
cargo test --locked -p mlx-ollama-daemon m4_24gb_bandwidth_impact_on_default_catalog -- --nocapture
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo check --locked -p mlx-hardware-fit -p mlx-model-fit --all-targets --target aarch64-apple-darwin
cargo check --locked -p mlx-hardware-fit -p mlx-model-fit --all-targets --target x86_64-apple-darwin
```

#### Impacto medido no motor de estimativas

Comparação controlada de 150 versus 120 GB/s com M4/24 e os 15 modelos do
catálogo padrão, quantização e contexto automáticos:

| Uso | Modelos que mudaram de posição | Exemplos |
| --- | ---: | --- |
| general | 6/15 | Llama 3 8B: 2º → 6º; LLaVA 7B: 4º → 2º |
| coding | 6/15 | Phi-3 Medium: 7º → 2º |
| creative | 0/15 | Scores de modelos maiores diminuem, sem alterar a ordem |
| analysis | 0/15 | Alguns scores diminuem, sem alterar a ordem |

| Modelo / uso general | TPS antes | TPS corrigido | Score antes | Score corrigido |
| --- | ---: | ---: | ---: | ---: |
| Llama 3 8B (Q8_0) | 24,375 | 19,500 | 87,750 | 85,750 |
| Gemma 2 9B (Q8_0) | 21,196 | 16,957 | 86,130 | 83,304 |
| Llama 3 70B (Q2_K) | 12,297 | 10,686 | 54,297 | 52,686 |

Para modelos totalmente na GPU, a estimativa de TPS cai exatamente 20%.
Offload parcial mistura banda CPU/GPU e cai menos. Scores têm saturação e
pesos por uso, por isso a mudança de TPS nem sempre muda o score ou a ordem.
Quants recomendadas, memória estimada e fit de memória permaneceram iguais
nos 60 pares comparados. O perfil manual M4/24 continua com 120 GB/s: a
correção faz o perfil detectado alcançar a simulação existente.

Estes resultados medem o **motor de estimativas em Linux x86_64**, não o
desempenho físico do Mac. A posição depende do catálogo e dos parâmetros.
Há uma heurística preexistente adicional: a banda de CPU usada para offload
parcial é 70 GB/s em x86_64 e 90 GB/s em ARM, escolhida na compilação. Portanto,
os números de offload parcial podem diferir ao repetir o experimento no Mac;
a queda de 20% nos modelos totalmente na GPU independe dessa diferença.
Essa heurística de CPU não foi alterada neste patch.

#### Verificações locais

Executado em Linux x86_64 com Rust/Cargo 1.99.1, usando `Cargo.lock` sem
alterações:

| Verificação | Resultado |
| --- | --- |
| `cargo test --locked --workspace` | 493 passaram; zero falhas |
| Pacotes diretamente envolvidos | hardware-fit: 21; model-fit: 15; daemon: 73 |
| Clippy do workspace, todos os targets, `-D warnings` | Sem avisos |
| `cargo fmt --all -- --check` e `git diff --check` | Passaram |
| `cargo check` das duas bibliotecas, todos os targets, aarch64-apple-darwin | Passou, incluindo compilação do teste macOS |
| Mesmo check em x86_64-apple-darwin | Passou, incluindo compilação do teste macOS |
| `real_m4_24gb_matches_simulator` | Não executado; requer o Mac real |

São nove testes de regressão novos executados no Linux, além do teste macOS
opcional. A suíte do workspace inclui os testes de integração existentes;
o fluxo gráfico da interface não foi executado nesta alteração.

### Validação que exige o Mac real

O ambiente de desenvolvimento desta correção é Linux x86_64. A checagem de
compilação cruzada inclui os testes específicos de macOS, mas não executa
sysctl, Metal ou inferência em um Mac. Nenhum benchmark foi realizado.

No Mac mini M4/24, nesta branch, execute:

```sh
/usr/sbin/sysctl -n machdep.cpu.brand_string
/usr/sbin/sysctl -n hw.memsize
cargo test --locked -p mlx-hardware-fit -p mlx-model-fit
cargo test --locked -p mlx-model-fit real_m4_24gb_matches_simulator -- --ignored --nocapture
```

O último teste é exclusivo de macOS e ignorado por padrão porque requer esse
hardware específico. Ele chama `detect_system(true)`, imprime o perfil real,
exige Apple M4, 10 núcleos, 24 GiB, Metal, 18 GiB estimados para GPU e 120 GB/s,
e compara rankings/perfis com a simulação. Esperado nas consultas manuais:
`Apple M4` e `25769803776` bytes.

Para conferir a integração na interface, reinicie o daemon compilado com a
correção e consulte `/api/hwfit/system?fresh=true`, evitando o cache de 24 horas.
Compare o painel detectado com a simulação Apple M4/24 nas mesmas opções de
uso/quant/contexto. `detected_at` e memória atualmente disponível naturalmente
diferem; a simulação reserva 90% de RAM disponível, enquanto a detecção lê o
estado real. Hoje o ranking utiliza RAM total, não RAM atualmente disponível.

A execução Rosetta precisa de validação separada com o daemon x86_64 em um
Mac ARM. Os testes simulados cobrem os sinais de host traduzido; a checagem de
compilação x86_64 não prova o comportamento do SO sob tradução.

### Limitações preservadas

- 120 GB/s é a banda publicada do M4, não throughput medido. A estimativa de TPS
  continua heurística e não equivale a um benchmark de inferência.
- A parcela de 75% para GPU continua uma aproximação, não uma leitura da memória
  efetivamente disponível via Metal.
- A tabela existente continua escolhendo o bin superior quando a memória não
  diferencia a configuração; resolver todos os bins requer mais sinais do SO.
- Não houve revisão geral da tabela de chips nem mudança no algoritmo de score.
- O caminho de fallback não conhece a banda física de um chip futuro.

### Fontes

- [PR #69](https://github.com/MLX-Pilot/MLX-Pilot/pull/69)
- [Issue #65](https://github.com/MLX-Pilot/MLX-Pilot/issues/65)
- [XNU: implementação ARM de machdep.cpu.brand_string](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/dev/arm64/sysctl.c)
- [Apple: Mac mini (2024), especificações técnicas](https://support.apple.com/pt-br/121555)
- [Apple: ambiente de tradução Rosetta](https://developer.apple.com/documentation/apple-silicon/about-the-rosetta-translation-environment)
