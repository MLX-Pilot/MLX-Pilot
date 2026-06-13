/* MLX Pilot — Daemon & native bridge (core).
 *
 * Shared network primitives: the Tauri `nativeInvoke` bridge, the `api()`
 * fetch wrapper against the local daemon, and the SSE stream decoder factory.
 * Depends only on state.js for the daemon URL and timeout constants.
 */

import { state, DEFAULT_DAEMON_URL, API_DEFAULT_TIMEOUT_MS } from './state.js';

export function nativeInvoke(command, args = {}) {
  const invoke = window.__TAURI__?.core?.invoke || window.__TAURI__?.tauri?.invoke;
  if (typeof invoke !== 'function') {
    return Promise.reject(new Error('Runtime Tauri indisponivel'));
  }
  return invoke(command, args);
}

export async function api(path, opts = {}) {
  const url = (state.daemonUrl || DEFAULT_DAEMON_URL) + path;
  const inferredTimeoutMs =
    path.startsWith('/chat')
    || path.startsWith('/agent/run')
    || path.startsWith('/catalog/downloads')
      ? 120000
      : API_DEFAULT_TIMEOUT_MS;
  const { timeoutMs = inferredTimeoutMs, headers: requestHeaders = {}, ...fetchOpts } = opts;
  const headers = { ...requestHeaders };

  if (fetchOpts.body != null && !Object.keys(headers).some(key => key.toLowerCase() === 'content-type')) {
    headers['Content-Type'] = 'application/json';
  }

  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), timeoutMs);

  let res;
  try {
    res = await fetch(url, {
      ...fetchOpts,
      headers,
      signal: controller.signal,
    });
  } catch (error) {
    if (error?.name === 'AbortError') {
      throw new Error(`Tempo limite ao acessar ${path}`);
    }
    throw error;
  } finally {
    clearTimeout(timeoutId);
  }

  if (res.status === 204 || res.status === 205) return null;
  if (!res.ok) {
    let msg = `HTTP ${res.status}`;
    try {
      const body = await res.json();
      if (body.error) msg = body.error_code ? `${body.error_code}: ${body.error}` : body.error;
    } catch { /* ok */ }
    throw new Error(msg);
  }
  const text = await res.text();
  if (!text) return null;
  try { return JSON.parse(text); } catch { return { message: text }; }
}

export function createStreamDecoder() {
  const Decoder = window.TextDecoder || globalThis.TextDecoder;
  if (!Decoder) throw new Error('Streaming indisponivel: TextDecoder nao encontrado');
  return new Decoder();
}
