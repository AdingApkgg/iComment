/** 传输层：包一层 fetch，把 problem+json 变成有类型的错误。 */

export type Problem = {
  code: string;
  message: string;
  status: number;
  fields: Record<string, string>;
};

export class ICommentError extends Error {
  constructor(readonly problem: Problem) {
    super(problem.message);
    this.name = "ICommentError";
  }

  get code(): string {
    return this.problem.code;
  }

  get status(): number {
    return this.problem.status;
  }
}

/** `next` 来自服务端，告诉调用方下一步该做什么。 */
export type NextStep = "wait" | "poll" | "reauthorize" | null;

export type Transport = {
  base: string;
  token?: string | (() => string | undefined);
  fetch?: typeof fetch;
};

export async function request<T>(
  transport: Transport,
  method: string,
  path: string,
  body?: unknown,
): Promise<{ data: T; next: NextStep }> {
  return send<T>(transport, method, path, body, {});
}

/** 请求体不是 JSON 时用（目前只有 multipart 上传）。 */
export async function requestRaw<T>(
  transport: Transport,
  method: string,
  path: string,
  body: FormData,
  headers: Record<string, string> = {},
): Promise<{ data: T; next: NextStep }> {
  return send<T>(transport, method, path, body, headers);
}

async function send<T>(
  transport: Transport,
  method: string,
  path: string,
  body: unknown,
  headers: Record<string, string>,
): Promise<{ data: T; next: NextStep }> {
  const token = typeof transport.token === "function" ? transport.token() : transport.token;
  const doFetch = transport.fetch ?? globalThis.fetch;
  const encoded = body instanceof FormData;

  const res = await doFetch(`${transport.base}${path}`, {
    method,
    headers: {
      accept: "application/json",
      ...(body === undefined || encoded ? {} : { "content-type": "application/json" }),
      ...(token ? { authorization: `Bearer ${token}` } : {}),
      ...headers,
    },
    body: body === undefined || encoded ? (body as FormData | undefined) : JSON.stringify(body),
  });

  if (res.status === 204) return { data: undefined as T, next: null };

  const payload = await res.json().catch(() => null);
  if (!res.ok) {
    const record = (payload ?? {}) as Partial<Problem> & { next?: NextStep };
    throw new ICommentError({
      code: record.code ?? "http_error",
      message: record.message ?? `${res.status} ${res.statusText}`,
      status: res.status,
      fields: record.fields ?? {},
    });
  }
  return { data: payload as T, next: (payload as { next?: NextStep } | null)?.next ?? null };
}
