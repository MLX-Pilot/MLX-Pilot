/* MLX Pilot — ES module entry point.
 *
 * Loads the front-end scripts as side-effect ES modules, preserving the
 * original execution order (app.js -> wave1.js -> wave5.js). This is the entry
 * step of the ESM modularization: behaviour is unchanged. Subsequent steps move
 * logic out of these files into focused js/core/ and js/features/ modules, which
 * this file then imports in the same dependency order.
 */
import '../app.js';
import '../wave1.js';
import '../wave5.js';
