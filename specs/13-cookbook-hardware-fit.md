# 13 — Cookbook / Hardware-Fit (Scan de Hardware + Recomendação/Serve de Modelos)

> **Tipo:** nova feature · **Esforço:** XL · **Depende de:** catálogo/download
> existente (`/catalog/*`).

## 1. Objetivo

Detectar o **hardware** da máquina (GPU/VRAM, RAM, CPU), pontuar modelos por
**adequação ("fit")** ao hardware, recomendar quais baixar, e oferecer **perfis de
serve** (llama.cpp) Qualidade/Equilíbrio/Velocidade. Resolve a dificuldade do
usuário em saber "qual modelo roda bem aqui?", aproveitando o catálogo/downloads
que já existem.

## 2. Contexto Técnico

- **Backend:** novos crates `crates/hardware-fit` (detecção) e `crates/model-fit`
  (scoring), + módulo `crates/daemon/src/hwfit_routes.rs`.
- **Detecção:** `sysinfo` (RAM/CPU cross-platform); GPU via shell-out a
  `nvidia-smi`/`rocm-smi` ou crate `nvml-wrapper` (NVIDIA). Tudo com fallback
  gracioso (CPU-only).
- **Scoring:** heurística considerando VRAM/RAM vs tamanho/quant (GGUF/FP8/AWQ),
  arquitetura (idade), suporte de backend e necessidade de mmproj/visão; cache do
  scan (TTL 24h) em SQLite.
- **Serve:** gerar perfis de parâmetros para o `llama.cpp` já gerenciado pelo daemon
  (context size, gpu-layers) por modelo/objetivo.
- **Catálogo:** reutilizar `/catalog/models` e `/catalog/downloads` existentes para
  baixar o modelo recomendado.
- **Frontend:** aba `Hardware` (padrão `wave1.js`): cards de hardware, simulador de
  hardware manual, tabela ranqueada de modelos (fit/backend badges + baixar), perfis
  de serve.

### Referência no Odysseus (exemplo para consulta)

- `routes/cookbook_routes.py`, `routes/cookbook_helpers.py`, `routes/hwfit_routes.py`,
  `services/hwfit/` — scan, scoring e serve.
- `static/js/cookbook.js`, `cookbook-hwfit.js`, `cookbookServe.js`,
  `cookbookRunning.js`, `cookbookDownload.js`, `cookbookSchedule.js` — UI do Cookbook.
- `src/model_discovery.py` — descoberta/ranqueamento de modelos.
- Baseado em [`llmfit`](https://github.com/AlexsJones/llmfit) (modelo de fit-scoring).

## 3. Regras de Negócio e Restrições

- **PODE:** detectar hardware, simular hardware manual, ranquear modelos, baixar via
  catálogo, gerar perfis de serve.
- **NÃO PODE:** instalar runtime de GPU do host nem editar configs do SO
  automaticamente (só diagnóstico + instruções).
- **NÃO PODE:** travar se não houver GPU — funcionar CPU-only com ranqueamento honesto.
- **NÃO PODE:** prometer que um modelo roda — exibir fit como estimativa, com aviso.
- **Restrição:** detecção multiplataforma (Windows/macOS/Linux) com degradação clara
  quando uma via não está disponível (ex.: MLX só Apple Silicon).
- **Restrição:** cache do scan com TTL; não rodar `nvidia-smi` a cada request.

## 4. Critérios de Aceite

- [ ] `GET /api/hwfit/system` retorna RAM/CPU e GPU/VRAM quando detectável (ou CPU-only).
- [ ] `GET /api/hwfit/models` retorna catálogo ranqueado por fit, com badges de
      backend/quant.
- [ ] Simulador de hardware manual altera o ranqueamento.
- [ ] Baixar a partir da recomendação usa o pipeline `/catalog/downloads` existente.
- [ ] `GET /api/hwfit/profiles` retorna perfis de serve (Qualidade/Equilíbrio/Velocidade).
- [ ] Sem GPU, tudo funciona e o ranqueamento reflete CPU/RAM.

## 5. Plano de Implementação

1. **Detecção:** crate `hardware-fit` com `sysinfo` + GPU (nvidia-smi/rocm-smi/nvml),
   normalizando num `HardwareProfile`.
2. **Scoring:** crate `model-fit` com heurística de fit (VRAM/RAM/quant/arch/backend).
3. **Cache** do scan (SQLite, TTL 24h).
4. **Endpoints** `/api/hwfit/{system,models,profiles}` + simulador manual.
5. **Integração com catálogo** para baixar recomendado.
6. **Perfis de serve** para o llama.cpp gerenciado.
7. **UI:** aba `Hardware` (cards, simulador, tabela ranqueada, perfis, baixar).
