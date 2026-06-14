/* MLX Pilot — Console capture (core/bootstrap).
 *
 * Installs the global console.* overrides + window error/rejection hooks that
 * mirror UI logs into the in-app console feed. Imported first by main.js so the
 * capture is armed before the feature modules emit their startup logs. Exposes
 * pushConsoleEntry, used by feature modules to record structured entries.
 */

import { nativeInvoke } from './api.js';
import { state } from './state.js';
import { renderConsole } from '../features/console.js';


  const originalConsole = {
    log: console.log.bind(console),
    info: console.info.bind(console),
    warn: console.warn.bind(console),
    error: console.error.bind(console),
  };

  function stringifyConsoleArg(value) {
    if (value instanceof Error) return value.stack || value.message;
    if (typeof value === 'string') return value;
    try {
      return JSON.stringify(value);
    } catch {
      return String(value);
    }
  }

  export function pushConsoleEntry(level, source, message) {
    const entry = {
      time: new Date().toISOString(),
      level: String(level || 'info').toLowerCase(),
      source: source || 'ui',
      message: String(message || '').replace(/\s+/g, ' ').trim(),
    };
    state.consoleEntries.push(entry);
    if (state.consoleEntries.length > 300) state.consoleEntries.shift();
    void nativeInvoke('desktop_log_append', {
      level: entry.level,
      message: `${entry.source}: ${entry.message}`,
    }).catch(() => {});
    renderConsole();
  }

  ['log', 'info', 'warn', 'error'].forEach((level) => {
    console[level] = (...args) => {
      originalConsole[level](...args);
      pushConsoleEntry(level === 'log' ? 'info' : level, 'ui', args.map(stringifyConsoleArg).join(' '));
    };
  });

  window.addEventListener('error', (event) => {
    pushConsoleEntry('error', 'window', `${event.message || 'Erro sem mensagem'} ${event.filename || ''}:${event.lineno || 0}`);
  });

  window.addEventListener('unhandledrejection', (event) => {
    pushConsoleEntry('error', 'promise', stringifyConsoleArg(event.reason || 'Promise rejeitada sem motivo'));
  });
