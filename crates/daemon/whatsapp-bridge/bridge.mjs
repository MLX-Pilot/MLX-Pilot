import { spawn } from "node:child_process";
import fsSync from "node:fs";
import fs from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import {
  DisconnectReason,
  fetchLatestBaileysVersion,
  makeCacheableSignalKeyStore,
  makeWASocket,
  useMultiFileAuthState,
} from "@whiskeysockets/baileys";

const DEFAULT_LOGIN_TIMEOUT_MS = 180_000;
const DEFAULT_CONNECT_TIMEOUT_MS = 30_000;
const POLL_INTERVAL_MS = 250;
const SCRIPT_PATH = fileURLToPath(import.meta.url);
const SCRIPT_DIR = path.dirname(SCRIPT_PATH);

function parseArgs(argv) {
  const out = {};
  for (let index = 0; index < argv.length; index += 1) {
    const item = argv[index];
    if (!item.startsWith("--")) {
      continue;
    }
    const key = item.slice(2).replace(/-/g, "_");
    const next = argv[index + 1];
    if (!next || next.startsWith("--")) {
      out[key] = true;
      continue;
    }
    out[key] = next;
    index += 1;
  }
  return out;
}

function toInt(value, fallback) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return fallback;
  }
  return Math.floor(parsed);
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function ensureDir(dirPath) {
  await fs.mkdir(dirPath, { recursive: true });
}

function authDirFor(sessionRoot) {
  return path.join(sessionRoot, "auth");
}

function sessionPathFor(sessionRoot) {
  return path.join(sessionRoot, "session.json");
}

function loginPidPathFor(sessionRoot) {
  return path.join(sessionRoot, "whatsapp-login.pid");
}

function credsPathFor(authDir) {
  return path.join(authDir, "creds.json");
}

function credsBackupPathFor(authDir) {
  return path.join(authDir, "creds.json.bak");
}

function buildSessionState({
  status,
  sessionRoot,
  qrCode = null,
  connectedAtEpochMs = null,
  disconnectedAtEpochMs = null,
}) {
  return {
    status,
    session_dir: sessionRoot,
    qr_code: qrCode,
    qr_image_data_url: null,
    connected_at_epoch_ms: connectedAtEpochMs,
    disconnected_at_epoch_ms: disconnectedAtEpochMs,
  };
}

async function writeSessionState(sessionRoot, state) {
  await ensureDir(sessionRoot);
  await fs.writeFile(sessionPathFor(sessionRoot), JSON.stringify(state, null, 2));
}

async function readSessionState(sessionRoot) {
  try {
    const raw = await fs.readFile(sessionPathFor(sessionRoot), "utf8");
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

async function removePath(targetPath) {
  await fs.rm(targetPath, { recursive: true, force: true });
}

async function readPid(pidPath) {
  try {
    const raw = (await fs.readFile(pidPath, "utf8")).trim();
    if (!raw) {
      return null;
    }
    const pid = Number(raw);
    return Number.isInteger(pid) && pid > 0 ? pid : null;
  } catch {
    return null;
  }
}

function isPidAlive(pid) {
  if (!pid) {
    return false;
  }
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function writePid(pidPath) {
  await fs.writeFile(pidPath, `${process.pid}\n`);
}

async function clearPidIfOwned(pidPath) {
  const pid = await readPid(pidPath);
  if (pid === process.pid) {
    await removePath(pidPath);
  }
}

async function killActiveLoginWorker(sessionRoot) {
  const pidPath = loginPidPathFor(sessionRoot);
  const pid = await readPid(pidPath);
  if (!pid) {
    return false;
  }
  if (!isPidAlive(pid)) {
    await removePath(pidPath);
    return false;
  }
  try {
    process.kill(pid, "SIGTERM");
  } catch {
    await removePath(pidPath);
    return false;
  }
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (!isPidAlive(pid)) {
      break;
    }
    await sleep(100);
  }
  if (isPidAlive(pid)) {
    try {
      process.kill(pid, "SIGKILL");
    } catch {
      // ignore
    }
  }
  await removePath(pidPath);
  return true;
}

function normalizeE164(rawValue) {
  const cleaned = String(rawValue ?? "")
    .replace(/^whatsapp:/i, "")
    .trim()
    .replace(/[^\d+]/g, "");
  if (!cleaned) {
    throw new Error("target vazio");
  }
  if (cleaned.startsWith("+")) {
    return `+${cleaned.slice(1)}`;
  }
  return `+${cleaned}`;
}

function toWhatsAppJid(target) {
  const raw = String(target ?? "").replace(/^whatsapp:/i, "").trim();
  if (!raw) {
    throw new Error("target vazio");
  }
  if (raw.includes("@")) {
    return raw;
  }
  const digits = normalizeE164(raw).replace(/\D/g, "");
  if (!digits) {
    throw new Error(`target invalido: ${target}`);
  }
  return `${digits}@s.whatsapp.net`;
}

function readCredsJsonRaw(filePath) {
  try {
    if (!fsSync.existsSync(filePath)) {
      return null;
    }
    const stats = fsSync.statSync(filePath);
    if (!stats.isFile() || stats.size <= 1) {
      return null;
    }
    return fsSync.readFileSync(filePath, "utf8");
  } catch {
    return null;
  }
}

function maybeRestoreCredsFromBackup(authDir) {
  try {
    const credsPath = credsPathFor(authDir);
    const backupPath = credsBackupPathFor(authDir);
    const raw = readCredsJsonRaw(credsPath);
    if (raw) {
      JSON.parse(raw);
      return;
    }
    const backupRaw = readCredsJsonRaw(backupPath);
    if (!backupRaw) {
      return;
    }
    JSON.parse(backupRaw);
    fsSync.copyFileSync(backupPath, credsPath);
  } catch {
    // ignore
  }
}

let credsSaveQueue = Promise.resolve();

function queueSaveCreds(authDir, saveCreds) {
  credsSaveQueue = credsSaveQueue
    .then(async () => {
      try {
        const credsPath = credsPathFor(authDir);
        const backupPath = credsBackupPathFor(authDir);
        const raw = readCredsJsonRaw(credsPath);
        if (raw) {
          JSON.parse(raw);
          fsSync.copyFileSync(credsPath, backupPath);
        }
      } catch {
        // ignore backup failures
      }
      await Promise.resolve(saveCreds());
    })
    .catch(() => {
      // ignore queued save failures
    });
}

async function webAuthExists(authDir) {
  maybeRestoreCredsFromBackup(authDir);
  try {
    const stats = await fs.stat(credsPathFor(authDir));
    if (!stats.isFile() || stats.size <= 1) {
      return false;
    }
    JSON.parse(await fs.readFile(credsPathFor(authDir), "utf8"));
    return true;
  } catch {
    return false;
  }
}

function readSelfJid(authDir) {
  try {
    const raw = fsSync.readFileSync(credsPathFor(authDir), "utf8");
    const parsed = JSON.parse(raw);
    const jid = parsed?.me?.id;
    return typeof jid === "string" && jid.trim() ? jid.trim() : null;
  } catch {
    return null;
  }
}

function getStatusCode(error) {
  return error?.output?.statusCode ?? error?.status ?? error?.data?.statusCode ?? null;
}

function formatError(error) {
  const boomMessage = error?.output?.payload?.message;
  if (typeof boomMessage === "string" && boomMessage.trim()) {
    return boomMessage.trim();
  }
  if (error instanceof Error && error.message.trim()) {
    return error.message.trim();
  }
  return String(error ?? "erro desconhecido");
}

function createLogger() {
  const logger = {
    level: "silent",
    trace() {},
    debug() {},
    info() {},
    warn() {},
    error() {},
    fatal() {},
    child() {
      return logger;
    },
  };
  return logger;
}

async function createSocket(authDir, { onQr } = {}) {
  await ensureDir(authDir);
  maybeRestoreCredsFromBackup(authDir);
  const { state, saveCreds } = await useMultiFileAuthState(authDir);
  let versionInfo = null;
  try {
    versionInfo = await fetchLatestBaileysVersion();
  } catch {
    versionInfo = null;
  }
  const logger = createLogger();
  const sock = makeWASocket({
    auth: {
      creds: state.creds,
      keys: makeCacheableSignalKeyStore(state.keys, logger),
    },
    ...(versionInfo?.version ? { version: versionInfo.version } : {}),
    logger,
    printQRInTerminal: false,
    browser: ["mlx-pilot", "daemon", "whatsapp"],
    syncFullHistory: false,
    markOnlineOnConnect: false,
  });

  sock.ev.on("creds.update", () => queueSaveCreds(authDir, saveCreds));
  sock.ev.on("connection.update", (update) => {
    if (typeof update?.qr === "string" && update.qr.trim()) {
      onQr?.(update.qr.trim());
    }
  });

  if (sock.ws && typeof sock.ws.on === "function") {
    sock.ws.on("error", () => {
      // ignore websocket noise
    });
  }

  return sock;
}

async function waitForConnection(sock, timeoutMs) {
  return await new Promise((resolve, reject) => {
    let finished = false;
    const timer = setTimeout(() => {
      if (finished) {
        return;
      }
      finished = true;
      reject(new Error("Timed out waiting for WhatsApp connection."));
    }, timeoutMs);

    const cleanup = () => {
      if (typeof sock.ev?.off === "function") {
        sock.ev.off("connection.update", handler);
      }
      clearTimeout(timer);
    };

    const handler = (update) => {
      if (finished) {
        return;
      }
      if (update?.connection === "open") {
        finished = true;
        cleanup();
        resolve(update);
        return;
      }
      if (update?.connection === "close") {
        finished = true;
        cleanup();
        reject(update?.lastDisconnect ?? new Error("Connection closed."));
      }
    };

    sock.ev.on("connection.update", handler);
  });
}

async function waitForSessionState(sessionRoot, timeoutMs, predicate) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() <= deadline) {
    const state = await readSessionState(sessionRoot);
    if (state && predicate(state)) {
      return state;
    }
    await sleep(POLL_INTERVAL_MS);
  }
  return null;
}

function emitJson(payload) {
  process.stdout.write(`${JSON.stringify(payload)}\n`);
}

async function loginStart({ sessionRoot, accountId, timeoutMs }) {
  await ensureDir(sessionRoot);
  const pidPath = loginPidPathFor(sessionRoot);
  const authDir = authDirFor(sessionRoot);
  const activePid = await readPid(pidPath);
  if (activePid && isPidAlive(activePid)) {
    const state = await waitForSessionState(sessionRoot, timeoutMs, (candidate) =>
      ["pending_qr", "connected", "not_logged_in", "logged_out", "error"].includes(
        candidate.status,
      ),
    );
    if (!state) {
      throw new Error("Timeout aguardando a sessao WhatsApp em andamento.");
    }
    return buildLoginResponseFromState(state);
  }

  await removePath(pidPath);

  if (await webAuthExists(authDir)) {
    const selfJid = readSelfJid(authDir);
    const state = buildSessionState({
      status: "connected",
      sessionRoot,
      connectedAtEpochMs: Date.now(),
    });
    await writeSessionState(sessionRoot, state);
    return {
      status: "connected",
      message: selfJid
        ? `WhatsApp ja vinculado (${selfJid}).`
        : "WhatsApp ja vinculado.",
    };
  }

  const child = spawn(
    process.execPath,
    [
      SCRIPT_PATH,
      "login-worker",
      "--session-root",
      sessionRoot,
      "--account-id",
      accountId,
      "--timeout-ms",
      String(Math.max(timeoutMs, DEFAULT_LOGIN_TIMEOUT_MS)),
    ],
    {
      cwd: SCRIPT_DIR,
      detached: true,
      stdio: "ignore",
    },
  );
  child.unref();

  const state = await waitForSessionState(sessionRoot, Math.max(timeoutMs, 10_000), (candidate) =>
    ["pending_qr", "connected", "not_logged_in", "logged_out", "error"].includes(candidate.status),
  );
  if (!state) {
    throw new Error("Timeout aguardando o QR code do WhatsApp.");
  }
  return buildLoginResponseFromState(state);
}

function buildLoginResponseFromState(state) {
  if (state.status === "pending_qr" && state.qr_code) {
    return {
      status: "pending_qr",
      message: "Escaneie o QR code no WhatsApp > Dispositivos conectados.",
      qr_code: state.qr_code,
    };
  }
  if (state.status === "connected") {
    return {
      status: "connected",
      message: "WhatsApp conectado com sucesso.",
    };
  }
  if (state.status === "logged_out") {
    return {
      status: "logged_out",
      message: "WhatsApp desconectado.",
    };
  }
  if (state.status === "error" && typeof state.error === "string") {
    throw new Error(state.error);
  }
  return {
    status: state.status || "not_logged_in",
    message: "WhatsApp ainda nao esta conectado.",
  };
}

async function loginWorker({ sessionRoot, timeoutMs }) {
  const pidPath = loginPidPathFor(sessionRoot);
  const authDir = authDirFor(sessionRoot);
  await ensureDir(authDir);
  await writePid(pidPath);

  let sock = null;
  let qrSeen = false;
  let connected = false;
  let restartAttempted = false;

  const updatePendingQr = async (qrCode) => {
    qrSeen = true;
    await writeSessionState(
      sessionRoot,
      buildSessionState({
        status: "pending_qr",
        sessionRoot,
        qrCode,
      }),
    );
  };

  try {
    sock = await createSocket(authDir, {
      onQr: async (qrCode) => {
        if (qrSeen) {
          return;
        }
        await updatePendingQr(qrCode);
      },
    });

    try {
      await waitForConnection(sock, timeoutMs);
    } catch (error) {
      const statusCode = getStatusCode(error);
      if (statusCode === 515 && !restartAttempted) {
        restartAttempted = true;
        try {
          sock.ws?.close();
        } catch {
          // ignore
        }
        sock = await createSocket(authDir);
        await waitForConnection(sock, Math.max(Math.floor(timeoutMs / 2), 5_000));
      } else {
        throw error;
      }
    }

    connected = true;
    await writeSessionState(
      sessionRoot,
      buildSessionState({
        status: "connected",
        sessionRoot,
        connectedAtEpochMs: Date.now(),
      }),
    );
  } catch (error) {
    const statusCode = getStatusCode(error);
    if (statusCode === DisconnectReason.loggedOut) {
      await removePath(authDir);
      await writeSessionState(
        sessionRoot,
        buildSessionState({
          status: "logged_out",
          sessionRoot,
          disconnectedAtEpochMs: Date.now(),
        }),
      );
    } else if (qrSeen) {
      await writeSessionState(
        sessionRoot,
        buildSessionState({
          status: "not_logged_in",
          sessionRoot,
          disconnectedAtEpochMs: Date.now(),
        }),
      );
    } else {
      const state = buildSessionState({
        status: "error",
        sessionRoot,
        disconnectedAtEpochMs: Date.now(),
      });
      state.error = formatError(error);
      await writeSessionState(sessionRoot, state);
    }
    throw error;
  } finally {
    if (sock) {
      setTimeout(() => {
        try {
          sock.ws?.close();
        } catch {
          // ignore
        }
      }, connected ? 500 : 0);
    }
    await clearPidIfOwned(pidPath);
  }
}

async function probe({ sessionRoot }) {
  const pid = await readPid(loginPidPathFor(sessionRoot));
  const session = await readSessionState(sessionRoot);
  const authDir = authDirFor(sessionRoot);

  if (pid && isPidAlive(pid) && session?.status === "pending_qr" && session.qr_code) {
    return {
      status: "pending_qr",
      message: "Aguardando leitura do QR code do WhatsApp.",
      qr_code: session.qr_code,
    };
  }

  if (await webAuthExists(authDir)) {
    const connectedAtEpochMs =
      typeof session?.connected_at_epoch_ms === "number"
        ? session.connected_at_epoch_ms
        : Date.now();
    await writeSessionState(
      sessionRoot,
      buildSessionState({
        status: "connected",
        sessionRoot,
        connectedAtEpochMs,
      }),
    );
    return {
      status: "healthy",
      message: "Credenciais do WhatsApp disponiveis e prontas para uso.",
    };
  }

  await writeSessionState(
    sessionRoot,
    buildSessionState({
      status: "not_logged_in",
      sessionRoot,
      disconnectedAtEpochMs: Date.now(),
    }),
  );
  return {
    status: "not_logged_in",
    message: "WhatsApp ainda nao foi vinculado para esta conta.",
  };
}

async function logout({ sessionRoot }) {
  await killActiveLoginWorker(sessionRoot);
  await removePath(authDirFor(sessionRoot));
  await writeSessionState(
    sessionRoot,
    buildSessionState({
      status: "logged_out",
      sessionRoot,
      disconnectedAtEpochMs: Date.now(),
    }),
  );
  return {
    status: "logged_out",
    message: "Sessao do WhatsApp removida.",
  };
}

async function sendMessage({ sessionRoot, target, message, timeoutMs }) {
  const pendingPid = await readPid(loginPidPathFor(sessionRoot));
  const session = await readSessionState(sessionRoot);
  if (pendingPid && isPidAlive(pendingPid) && session?.status === "pending_qr") {
    throw new Error("Ainda aguardando o scan do QR code do WhatsApp.");
  }

  const authDir = authDirFor(sessionRoot);
  if (!(await webAuthExists(authDir))) {
    throw new Error("WhatsApp account is not logged in");
  }

  let sock = null;
  try {
    sock = await createSocket(authDir);
    await waitForConnection(sock, timeoutMs);
    const result = await sock.sendMessage(toWhatsAppJid(target), { text: message });
    await writeSessionState(
      sessionRoot,
      buildSessionState({
        status: "connected",
        sessionRoot,
        connectedAtEpochMs:
          typeof session?.connected_at_epoch_ms === "number"
            ? session.connected_at_epoch_ms
            : Date.now(),
      }),
    );
    return {
      message_id:
        typeof result === "object" && result && result.key && typeof result.key.id === "string"
          ? result.key.id
          : null,
    };
  } finally {
    if (sock) {
      setTimeout(() => {
        try {
          sock.ws?.close();
        } catch {
          // ignore
        }
      }, 500);
    }
  }
}

async function main() {
  const [command = "", ...rest] = process.argv.slice(2);
  const args = parseArgs(rest);
  const rawSessionRoot = String(args.session_root ?? "").trim();
  if (!rawSessionRoot) {
    throw new Error("missing --session-root");
  }
  const sessionRoot = path.resolve(rawSessionRoot);
  const accountId = String(args.account_id ?? "default");

  if (command === "login-start") {
    emitJson(
      await loginStart({
        sessionRoot,
        accountId,
        timeoutMs: toInt(args.timeout_ms, DEFAULT_CONNECT_TIMEOUT_MS),
      }),
    );
    return;
  }

  if (command === "login-worker") {
    await loginWorker({
      sessionRoot,
      timeoutMs: toInt(args.timeout_ms, DEFAULT_LOGIN_TIMEOUT_MS),
    });
    return;
  }

  if (command === "probe") {
    emitJson(await probe({ sessionRoot }));
    return;
  }

  if (command === "logout") {
    emitJson(await logout({ sessionRoot }));
    return;
  }

  if (command === "send") {
    emitJson(
      await sendMessage({
        sessionRoot,
        target: String(args.target ?? ""),
        message: String(args.message ?? ""),
        timeoutMs: toInt(args.timeout_ms, DEFAULT_CONNECT_TIMEOUT_MS),
      }),
    );
    return;
  }

  throw new Error(`unknown command: ${command}`);
}

main().catch((error) => {
  process.stderr.write(`${formatError(error)}\n`);
  process.exit(1);
});
