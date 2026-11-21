/** 服务端已经过白名单裁剪，这里只做类型包装。 */

export type ReactionKind = "up" | "heart" | "haha" | "thinking" | "party";

export type ReactionTally = {
  counts: Partial<Record<ReactionKind, number>>;
  mine: ReactionKind[];
};

export type Comment = {
  id: string;
  parent_id: string | null;
  depth: number;
  body_html: string;
  author_name: string | null;
  author_avatar: string | null;
  author_kind: string;
  status: string;
  identity_unverified: boolean;
  created_at: string;
  reactions: ReactionTally;
};

export type Page = {
  comments: Comment[];
  next_cursor: string | null;
  total: number;
};

export type SiteMeta = {
  origin: string;
  name: string | null;
  require_approval: boolean;
  challenge_mode: string;
  turnstile_site_key: string | null;
  max_depth: number;
  max_body_bytes: number;
};

export type Created = {
  comment: Comment | null;
  created: boolean;
  status: string;
  message: string | null;
};

export type Stored = {
  id: string;
  url: string;
  markdown: string;
  width: number;
  height: number;
};

export class ApiError extends Error {
  constructor(
    readonly code: string,
    message: string,
    readonly status: number,
    readonly fields: Record<string, string> = {},
  ) {
    super(message);
  }
}

export class Api {
  constructor(
    private readonly base: string,
    private readonly siteKey: string,
  ) {}

  private async call<T>(path: string, init: RequestInit = {}): Promise<T> {
    const res = await fetch(`${this.base}${path}`, {
      ...init,
      headers: {
        "content-type": "application/json",
        "x-site-key": this.siteKey,
        ...init.headers,
      },
      credentials: "omit",
    });
    if (res.status === 204) return undefined as T;
    const body = await res.json().catch(() => null);
    if (!res.ok) {
      const code = (body as { code?: string } | null)?.code ?? "unknown_error";
      const fields = (body as { fields?: Record<string, string> } | null)?.fields ?? {};
      throw new ApiError(
        code,
        (body as { message?: string } | null)?.message ?? res.statusText,
        res.status,
        fields,
      );
    }
    return body as T;
  }

  meta(path: string) {
    return this.call<SiteMeta>(`/site?path=${encodeURIComponent(path)}`, { method: "GET" });
  }

  page(path: string, cursor?: string | null) {
    const q = new URLSearchParams({ path, limit: "40" });
    if (cursor) q.set("cursor", cursor);
    return this.call<Page>(`/comments?${q}`, { method: "GET" });
  }

  post(path: string, payload: Record<string, unknown>) {
    return this.call<Created>("/comments", {
      method: "POST",
      body: JSON.stringify({ path, ...payload }),
    });
  }

  react(id: string, kind: ReactionKind, on: boolean) {
    return this.call<ReactionTally>(`/comments/${id}/reactions/${kind}`, {
      method: on ? "POST" : "DELETE",
    });
  }

  /** 上传一张图，回可以插进正文的 Markdown 片段。 */
  async upload(file: Blob): Promise<Stored> {
    const form = new FormData();
    form.append("file", file, file instanceof File ? file.name : "paste.png");
    const res = await fetch(`${this.base}/uploads`, {
      method: "POST",
      headers: { "x-site-key": this.siteKey },
      body: form,
      credentials: "omit",
    });
    const body = await res.json().catch(() => null);
    if (!res.ok) {
      throw new ApiError(
        (body as { code?: string } | null)?.code ?? "unknown_error",
        (body as { message?: string } | null)?.message ?? res.statusText,
        res.status,
      );
    }
    return body as Stored;
  }
}
