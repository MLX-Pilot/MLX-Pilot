/* MLX Pilot — DOM & formatting utilities (core).
 *
 * Pure, dependency-free helpers shared across modules: HTML escaping and
 * human-readable formatting of bytes / counts / durations, plus the model
 * icon lookup. No app state, no API calls.
 */

export function esc(s) { if (!s) return ''; const d = document.createElement('div'); d.textContent = String(s); return d.innerHTML; }
export function fmtBytes(b) { if (b >= 1e9) return (b / 1e9).toFixed(1) + ' GB'; if (b >= 1e6) return (b / 1e6).toFixed(1) + ' MB'; return (b / 1e3).toFixed(0) + ' KB'; }
export function fmtNum(n) { if (n >= 1e6) return (n / 1e6).toFixed(1) + 'M'; if (n >= 1e3) return (n / 1e3).toFixed(1) + 'K'; return String(n); }
export function fmtDuration(s) { if (s < 60) return s + 's'; if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`; return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`; }
export function modelIcon(id) { const l = (id || '').toLowerCase(); if (l.includes('llama')) return 'llama'; if (l.includes('mistral')) return 'mistral'; if (l.includes('qwen')) return 'qwen'; if (l.includes('deepseek')) return 'deepseek'; if (l.includes('phi')) return 'phi'; return 'llama'; }
