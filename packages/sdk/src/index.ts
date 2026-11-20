/**
 * iComment agent SDK。
 *
 * ```ts
 * const client = createClient({ base: "https://comments.example.com/api/v1", token });
 * await client.createSite({ origin: "blog.example.com" });
 * ```
 */

import { Client } from "./client.js";
import type { Transport } from "./transport.js";

export type { CreateCommentInput, CreateSiteInput, UpdateSiteInput } from "./client.js";
export { Client } from "./client.js";
export { ICommentError, type NextStep, type Problem, type Transport } from "./transport.js";
export type * from "./types.js";

export type ClientOptions = Omit<Transport, "fetch"> & { fetch?: typeof fetch };

/** 组装一个带令牌的客户端；令牌既可以给值也可以给函数（便于轮换）。 */
export function createClient(options: ClientOptions): Client {
  return new Client(options);
}
