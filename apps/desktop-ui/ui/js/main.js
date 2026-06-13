/* MLX Pilot — ES module entry point.
 *
 * Imports the application's ES modules in the SAME order their code ran in the
 * original monolith, so module-load side effects (console capture, splash,
 * startup, event-listener wiring) fire in the exact original sequence:
 *   app.js (shell + console capture) -> feature modules (in file order) ->
 *   wave1.js -> wave5.js.
 * Core modules (js/core/*) are pulled in transitively by the feature modules
 * that import them. Behaviour is unchanged from the pre-modularization build.
 */
import '../app.js';
import './features/providers.js';
import './features/runtime.js';
import './features/settings.js';
import './features/models.js';
import './features/chat.js';
import './features/agent.js';
import './features/console.js';
import './core/router.js';
import './features/agent-shortcuts.js';
import './features/ui-bindings.js';
import '../wave1.js';
import '../wave5.js';
