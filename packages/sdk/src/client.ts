/** 资源方法：全走 transport，错误统一抛 ICommentError。 */

import { request, requestRaw, type Transport } from "./transport.js";
import type {
  AuditEvent,
  Comment,
  CommentPage,
  DeviceStart,
  Me,
  ModerateAction,
  SignedIdentity,
  Site,
  SiteCreated,
  Stats,
  Stored,
  Thread,
  TokenIssued,
  VerifyResult,
} from "./types.js";

export type CreateSiteInput = {
  origin: string;
  name?: string | null;
  /** 建站时就带上黑名单，省一次更新 */
  blocked_domains?: string[];
};

export type UpdateSiteInput = {
  name?: string | null;
  require_approval?: boolean;
  challenge_mode?: "off" | "always" | "suspicious";
  /** 域名黑名单，命中即进垃圾箱；传 `[]` 清空 */
  blocked_domains?: string[];
};

export type CreateCommentInput = {
  path: string;
  body_md: string;
  parent_id?: string | null;
  author_name?: string | null;
  author_email?: string | null;
};

export class Client {
  constructor(private readonly transport: Transport) {}

  me(): Promise<Me> {
    return request<Me>(this.transport, "GET", "/agent/me").then((r) => r.data);
  }

  async startDeviceFlow(): Promise<DeviceStart> {
    const { data } = await request<DeviceStart>(this.transport, "POST", "/agent/device", {});
    return data;
  }

  /** device flow 第二轮：服务端说 pending 时由调用方自己 sleep(interval)。 */
  async pollDeviceFlow(deviceCode: string): Promise<TokenIssued | null> {
    try {
      const { data } = await request<TokenIssued>(this.transport, "POST", "/agent/token", {
        device_code: deviceCode,
      });
      return data;
    } catch (err) {
      const code = (err as { code?: string }).code;
      if (code === "device_authorization_pending") return null;
      throw err;
    }
  }

  /** 不传 tokenId 就撤掉当前这个 token。 */
  revokeToken(tokenId?: string): Promise<void> {
    const body = tokenId ? { token_id: tokenId } : {};
    return request<void>(this.transport, "POST", "/agent/tokens/revoke", body).then((r) => r.data);
  }

  async listSites(): Promise<Site[]> {
    const { data } = await request<{ sites: Site[] }>(this.transport, "GET", "/agent/sites");
    return data.sites;
  }

  async createSite(input: CreateSiteInput): Promise<SiteCreated> {
    const { data } = await request<SiteCreated>(this.transport, "POST", "/agent/sites", input);
    return data;
  }

  getSite(siteId: string): Promise<Site> {
    return request<Site>(this.transport, "GET", `/agent/sites/${siteId}`).then((r) => r.data);
  }

  updateSite(siteId: string, input: UpdateSiteInput): Promise<Site> {
    return request<Site>(this.transport, "PATCH", `/agent/sites/${siteId}`, input).then(
      (r) => r.data,
    );
  }

  deleteSite(siteId: string): Promise<void> {
    return request<void>(this.transport, "DELETE", `/agent/sites/${siteId}`).then((r) => r.data);
  }

  async listComments(
    siteId: string,
    options: { status?: string; cursor?: string; limit?: number } = {},
  ): Promise<CommentPage> {
    const query = new URLSearchParams();
    if (options.status) query.set("status", options.status);
    if (options.cursor) query.set("cursor", options.cursor);
    if (options.limit) query.set("limit", String(options.limit));
    const { data } = await request<CommentPage>(
      this.transport,
      "GET",
      `/agent/sites/${siteId}/comments?${query}`,
    );
    return data;
  }

  /** 以站主身份直发，跳过审核。 */
  createComment(siteId: string, input: CreateCommentInput): Promise<{ comment: Comment }> {
    return request<{ comment: Comment }>(
      this.transport,
      "POST",
      `/agent/sites/${siteId}/comments`,
      input,
    ).then((r) => r.data);
  }

  moderate(siteId: string, commentId: string, action: ModerateAction): Promise<Comment> {
    return request<{ comment: Comment }>(
      this.transport,
      "POST",
      `/agent/sites/${siteId}/comments/${commentId}/moderate`,
      { action },
    ).then((r) => r.data.comment);
  }

  /** 审核留痕。传 commentId 只看那一条的历史。 */
  async auditLog(siteId: string, commentId?: string): Promise<AuditEvent[]> {
    const suffix = commentId ? `?comment=${commentId}` : "";
    const { data } = await request<{ events: AuditEvent[] }>(
      this.transport,
      "GET",
      `/agent/sites/${siteId}/audit${suffix}`,
    );
    return data.events;
  }

  async listThreads(siteId: string): Promise<Thread[]> {
    const { data } = await request<{ threads: Thread[] }>(
      this.transport,
      "GET",
      `/agent/sites/${siteId}/threads`,
    );
    return data.threads;
  }

  /** 服务端抓页面验收安装：返回四要素齐全 / 缺什么 / 为什么失败。 */
  verifyInstall(siteId: string, url: string): Promise<VerifyResult> {
    return request<VerifyResult>(this.transport, "POST", `/agent/sites/${siteId}/verify`, {
      url,
    }).then((r) => r.data);
  }

  /** 签一段短时身份给前端，带上即可免挑战、免待审。 */
  signIdentity(siteId: string, uid: string, name?: string | null): Promise<SignedIdentity> {
    return request<SignedIdentity>(this.transport, "POST", `/agent/sites/${siteId}/identity`, {
      body: { uid, name: name ?? null },
    }).then((r) => r.data);
  }

  /** 上传一张图（site 或 agent 皆可，走公开口）；返回可插进 `body_md` 的片段。 */
  async upload(siteKey: string, file: Blob, filename = "paste.png"): Promise<Stored> {
    const form = new FormData();
    form.append("file", file, filename);
    const { data } = await requestRaw<Stored>(this.transport, "POST", "/uploads", form, {
      "x-site-key": siteKey,
    });
    return data;
  }

  stats(siteId: string): Promise<Stats> {
    return request<Stats>(this.transport, "GET", `/agent/sites/${siteId}/stats`).then(
      (r) => r.data,
    );
  }
}
