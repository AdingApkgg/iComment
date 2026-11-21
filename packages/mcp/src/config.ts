/** 凭据与地址解析：token 存 ~/.config/icomment/token，权限 600。 */

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";

const DEFAULT_BASE = "http://127.0.0.1:8080/api";

function tokenPath(): string {
  const root = process.env.XDG_CONFIG_HOME ?? join(homedir(), ".config");
  return join(root, "icomment", "token");
}

export function apiBase(): string {
  return process.env.IC_API_BASE ?? DEFAULT_BASE;
}

/** 服务根（去掉 `/api`），embed 链接和 /llms.txt 都挂在上面。 */
export function serviceOrigin(): string {
  return apiBase().replace(/\/api\/?$/, "");
}

export function loadToken(): string | undefined {
  if (process.env.IC_TOKEN) return process.env.IC_TOKEN;
  try {
    return readFileSync(tokenPath(), "utf8").trim() || undefined;
  } catch {
    return undefined;
  }
}

export function saveToken(token: string): void {
  const path = tokenPath();
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  writeFileSync(path, `${token}\n`, { mode: 0o600 });
}
