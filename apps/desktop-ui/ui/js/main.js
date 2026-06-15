/* MLX Pilot — ES module entry point.
 *
 * Imports the application's ES modules in the SAME order their code ran in the
 * original monolith, so module-load side effects (console capture, splash,
 * startup, event-listener wiring) fire in the exact original sequence:
 *   console-capture (console hooks) -> core feature modules (in file order) ->
 *   the former wave1 features (presets, memory, compare, history) ->
 *   the former wave5 features (research, hardware).
 * Core modules (js/core/*) are pulled in transitively by the feature modules
 * that import them. Behaviour is unchanged from the pre-modularization build.
 */
import './core/console-capture.js';
import './features/providers.js';
import './features/runtime.js';
import './features/settings.js';
import './features/models.js';
import './features/chat.js';
import './features/agent.js';
import './features/console.js';
import './features/monitor.js';
import './core/router.js';
import './features/agent-shortcuts.js';
import './features/ui-bindings.js';
import './features/presets.js';
import './features/memory.js';
import './features/compare.js';
import './features/history.js';
import './features/research.js';
import './features/hardware.js';
