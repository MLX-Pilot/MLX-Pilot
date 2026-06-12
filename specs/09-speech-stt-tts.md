# 09 — Speech (STT e TTS)

> **Tipo:** nova feature · **Esforço:** L · **Depende de:** — (Tauri p/ áudio nativo).

## 1. Objetivo

Adicionar **fala para texto** (microfone → transcrição, preenchendo o chat) e
**texto para fala** (ler respostas da IA em voz alta). Resolve a ausência de
interação por voz, melhorando acessibilidade e a experiência de assistente.

## 2. Contexto Técnico

- **STT (local, CPU):** crate `whisper-rs` (ggml, sem PyTorch) com modelo
  whisper pequeno (`base`/`small`) baixado sob demanda; atrás de feature flag.
  Captura de microfone via `cpal` + `hound` (WAV) no lado Tauri.
- **TTS:** crate `tts` (usa vozes nativas do SO — SAPI no Windows, etc.); zero
  download. Fallback: Web Speech API do webview do navegador.
- **Backend:** módulo `crates/daemon/src/speech.rs`: `POST /api/speech/transcribe`
  (áudio → texto) e `POST /api/speech/synthesize` (texto → áudio/па playback);
  cache de áudio + `GET /api/speech/cache/clear`.
- **Frontend:** botão de microfone no input do Chat (preenche `#chat-input`) e
  botão "ouvir" em mensagens do assistente; ajustes de voz/idioma em Settings.

### Referência no Odysseus (exemplo para consulta)

- `routes/stt_routes.py`, `services/stt/` — transcrição (faster-whisper opcional).
- `routes/tts_routes.py`, `services/tts/`, `static/js/tts-ai.js` — síntese de fala e UI.
- README do Odysseus: `faster-whisper` listado como dependência opcional (modelo
  de "feature opcional via flag").

## 3. Regras de Negócio e Restrições

- **PODE:** transcrever áudio enviado; sintetizar fala de um texto; escolher
  voz/idioma; usar fallback Web Speech quando o backend não suportar.
- **NÃO PODE:** enviar áudio para serviços remotos por padrão (processar local).
- **NÃO PODE:** baixar modelo Whisper automaticamente sem feature flag/aviso; o
  app funciona sem STT (botão desabilitado com tooltip).
- **NÃO PODE:** exceder limite de tamanho/duração de áudio (config por env).
- **Restrição:** captura de microfone deve usar o caminho nativo do Tauri (permissão
  do SO), não depender só do navegador.

## 4. Critérios de Aceite

- [ ] `POST /api/speech/transcribe` retorna texto de um WAV de teste (com feature on).
- [ ] `POST /api/speech/synthesize` produz áudio audível de um texto.
- [ ] Botão de microfone no Chat grava e preenche o input com a transcrição.
- [ ] Botão "ouvir" lê uma resposta do assistente em voz alta.
- [ ] Sem o modelo/feature, STT degrada com aviso; TTS usa voz do SO ou Web Speech.
- [ ] `cargo build` verde com e sem `--features speech`.

## 5. Plano de Implementação

1. **TTS primeiro** (mais simples): integrar crate `tts`; endpoint `synthesize`;
   botão "ouvir" no front (fallback Web Speech).
2. **Captura de áudio (Tauri):** comando nativo para gravar do microfone → WAV.
3. **STT:** integrar `whisper-rs` (feature flag); endpoint `transcribe`;
   documentar download do modelo.
4. **UI do microfone:** botão no input do Chat (estado gravando/parado) → transcreve
   → preenche `#chat-input`.
5. **Cache + settings:** cache de áudio TTS; ajustes de voz/idioma/limites em Settings.
