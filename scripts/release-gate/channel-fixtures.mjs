// Protocol peers for the real daemon. Every socket is confined to loopback.
import net from "node:net";
import { startJsonServer } from "./_lib.mjs";

export async function startBotFixture(telegramToken) {
  let sequence = 0;
  return startJsonServer(({ req, body }) => {
    if (req.method === "POST" && req.url === `/bot${telegramToken}/getMe`)
      return { body: { ok: true, result: { id: 123, is_bot: true, first_name: "Fixture" } } };
    if (req.method === "POST" && req.url === `/bot${telegramToken}/sendMessage` && body?.text && body.chat_id)
      return { body: { ok: true, result: { message_id: ++sequence } } };
    if (req.headers.authorization === "Bot fixture-discord") {
      if (req.method === "GET" && req.url === "/api/v10/users/@me")
        return { body: { id: "123", bot: true } };
      if (req.method === "POST" && req.url === "/api/v10/channels/212345678901234567/messages" && body?.content)
        return { body: { id: String(++sequence) } };
    }
    if (req.method === "POST" && req.headers.authorization === "Bearer fixture-slack") {
      if (req.url === "/api/auth.test") return { body: { ok: true, user_id: "U123" } };
      if (req.url === "/api/chat.postMessage" && body?.channel === "C123" && body.text)
        return { body: { ok: true, ts: `1234.${++sequence}` } };
    }
    return { status: 401, body: { ok: false, error: "invalid_auth" } };
  }, "token-bot-fixture");
}

export function startMatrixFixture() {
  return startJsonServer(({ req, body }) => {
    const base = "/_matrix/client/v3";
    if (req.headers.authorization !== "Bearer fixture-matrix") return { status: 401 };
    if (req.method === "GET" && req.url === base + "/account/whoami")
      return { body: { user_id: "@test:example.org" } };
    if (req.method === "GET" && req.url === base + "/directory/room/%23room%3Aexample.org")
      return { body: { room_id: "!room:example.org" } };
    if (req.method === "PUT" && req.url.startsWith(base + "/rooms/%21room%3Aexample.org/send/m.room.message/") && body?.msgtype === "m.text")
      return { body: { event_id: "$fixture" } };
    return { status: 404 };
  }, "matrix-fixture");
}

export async function startIrcFixture() {
  const lines = [];
  const server = net.createServer(socket => {
    socket.setEncoding("utf8");
    let pending = "";
    socket.on("data", data => {
      pending += data;
      const received = pending.split("\r\n");
      pending = received.pop();
      lines.push(...received);
    });
    socket.on("error", () => socket.destroy());
  });
  await new Promise((resolve, reject) => server.once("error", reject).listen(0, "127.0.0.1", resolve));
  return { lines, port: server.address().port, close: () => new Promise(resolve => server.close(resolve)) };
}
