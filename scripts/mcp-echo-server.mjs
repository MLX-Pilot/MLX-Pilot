// Servidor MCP minimo por stdio, usado para verificar o cliente do mlx-flow
// sem depender de download de pacote. Fala JSON-RPC 2.0 linha a linha.
import readline from "node:readline";

const rl = readline.createInterface({ input: process.stdin });

function reply(id, result) {
  process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id, result }) + "\n");
}

function replyError(id, code, message) {
  process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id, error: { code, message } }) + "\n");
}

rl.on("line", (line) => {
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    return;
  }

  const { id, method, params } = message;

  if (method === "initialize") {
    reply(id, {
      protocolVersion: "2024-11-05",
      capabilities: { tools: {} },
      serverInfo: { name: "echo-de-teste", version: "1.0.0" },
    });
    // Um log espontaneo, para confirmar que o cliente ignora notificacoes.
    process.stdout.write(
      JSON.stringify({ jsonrpc: "2.0", method: "notifications/message", params: { level: "info" } }) + "\n",
    );
    return;
  }

  if (method === "notifications/initialized") return;

  if (method === "tools/list") {
    reply(id, {
      tools: [
        {
          name: "eco",
          description: "Devolve o texto recebido em maiusculas",
          inputSchema: {
            type: "object",
            properties: { texto: { type: "string" } },
            required: ["texto"],
          },
        },
        {
          name: "somar",
          description: "Soma dois numeros",
          inputSchema: {
            type: "object",
            properties: { a: { type: "number" }, b: { type: "number" } },
          },
        },
      ],
    });
    return;
  }

  if (method === "tools/call") {
    const { name, arguments: args = {} } = params || {};
    if (name === "eco") {
      reply(id, { content: [{ type: "text", text: String(args.texto ?? "").toUpperCase() }] });
      return;
    }
    if (name === "somar") {
      reply(id, { content: [{ type: "text", text: String(Number(args.a || 0) + Number(args.b || 0)) }] });
      return;
    }
    reply(id, { isError: true, content: [{ type: "text", text: `ferramenta desconhecida: ${name}` }] });
    return;
  }

  replyError(id, -32601, `Method not found: ${method}`);
});
