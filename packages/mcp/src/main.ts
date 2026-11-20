#!/usr/bin/env node

/** 入口：默认走 stdio；`--http` 起可挂反向代理的 streamable HTTP。 */

import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { createMcpHandler } from "@modelcontextprotocol/server";
import { serveStdio } from "@modelcontextprotocol/server/stdio";
import { buildServer } from "./server.js";

const HTTP_PATH = "/mcp";

async function main(): Promise<void> {
  if (!process.argv.includes("--http")) {
    serveStdio(() => buildServer());
    return;
  }

  const port = Number(process.env.IC_MCP_PORT ?? 8787);
  const handler = createMcpHandler(() => buildServer());

  const server = createServer(async (req: IncomingMessage, res: ServerResponse) => {
    if ((req.url ?? "").split("?")[0] !== HTTP_PATH) {
      res.writeHead(404, { "content-type": "text/plain" }).end("not found");
      return;
    }
    const headers: Record<string, string> = {};
    for (const [key, value] of Object.entries(req.headers)) {
      if (typeof value === "string") headers[key] = value;
      else if (Array.isArray(value)) headers[key] = value.join(", ");
    }
    const hasBody = req.method !== "GET" && req.method !== "DELETE";
    const chunks: Buffer[] = [];
    if (hasBody) {
      for await (const chunk of req) chunks.push(chunk as Buffer);
    }
    const response = await handler.fetch(
      new Request(`http://127.0.0.1:${port}${req.url}`, {
        method: req.method,
        headers,
        body: hasBody ? Buffer.concat(chunks) : undefined,
      }),
    );
    res.writeHead(response.status, Object.fromEntries(response.headers));
    res.end(Buffer.from(await response.arrayBuffer()));
  });

  server.listen(port, "127.0.0.1", () => {
    process.stderr.write(`icomment-mcp listening on http://127.0.0.1:${port}${HTTP_PATH}\n`);
  });
}

main().catch((err: unknown) => {
  process.stderr.write(`${err instanceof Error ? (err.stack ?? err.message) : String(err)}\n`);
  process.exitCode = 1;
});
