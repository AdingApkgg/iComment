import { css, html, LitElement, nothing, type TemplateResult } from "lit";
import { live } from "lit/directives/live.js";
import { repeat } from "lit/directives/repeat.js";
import { Api, ApiError, type Comment, type SiteMeta } from "./api.js";

const PAGE_SIZE_HINT = 40;

/** 脚本自己的地址：以它推断 API 前缀，避免依赖 import.meta（要让普通 script 也能跑）。 */
const SELF_SRC = (document.currentScript as HTMLScriptElement | null)?.src ?? "";

type View = {
  meta: SiteMeta | null;
  comments: Comment[];
  next: string | null;
  total: number;
  loading: boolean;
  error: string | null;
  replyTo: Comment | null;
  name: string;
  email: string;
  body: string;
  sending: boolean;
  notice: string | null;
};

const blank = (): View => ({
  meta: null,
  comments: [],
  next: null,
  total: 0,
  loading: true,
  error: null,
  replyTo: null,
  name: "",
  email: "",
  body: "",
  sending: false,
  notice: null,
});

/** 与核心的 CommentView 对齐；深度由客户端按 parent 链算，免得信服务端。 */
function byParent(list: Comment[]): Map<string | null, Comment[]> {
  const map = new Map<string | null, Comment[]>([[null, []]]);
  for (const c of list) {
    const key = c.parent_id ?? null;
    const bucket = map.get(key) ?? [];
    bucket.push(c);
    map.set(key, bucket);
  }
  return map;
}

export class CommentWidget extends LitElement {
  static override properties = {
    siteKey: { type: String, attribute: "site-key" },
    apiBase: { type: String, attribute: "api-base" },
    path: { type: String, attribute: "path" },
    theme: { type: String, attribute: "theme" },
    _view: { state: true },
  };

  static override styles = css`
    :host {
      --ic-fg: #10131a;
      --ic-muted: #646d7c;
      --ic-line: #e2e6ee;
      --ic-bg: #fff;
      --ic-brand: #10131a;
      --ic-ok: #0b7a4b;
      --ic-bad: #b42318;
      display: block;
      color: var(--ic-fg);
      font: 15px/1.65 ui-sans-serif, system-ui, -apple-system, "Segoe UI", "PingFang SC",
        "Noto Sans CJK SC", sans-serif;
    }
    :host([hidden]) {
      display: none;
    }
    :host {
      @media (prefers-color-scheme: dark) {
        --ic-fg: #e8ecf3;
        --ic-muted: #98a2b3;
        --ic-line: #2a313d;
        --ic-bg: #10131a;
        --ic-brand: #fff;
      }
    }
    :host([theme="dark"]) {
      --ic-fg: #e8ecf3;
      --ic-muted: #98a2b3;
      --ic-line: #2a313d;
      --ic-bg: #10131a;
      --ic-brand: #fff;
    }
    h2 {
      margin: 0 0 0.75rem;
      font-size: 1rem;
      font-weight: 650;
      letter-spacing: -0.01em;
    }
    .count {
      color: var(--ic-muted);
      font-weight: 400;
      font-variant-numeric: tabular-nums;
    }
    fieldset {
      border: 1px solid var(--ic-line);
      border-radius: 10px;
      margin: 0 0 1.5rem;
      padding: 0.8rem;
      display: grid;
      gap: 0.6rem;
    }
    input,
    textarea {
      width: 100%;
      box-sizing: border-box;
      font: inherit;
      color: inherit;
      background: var(--ic-bg);
      border: 1px solid var(--ic-line);
      border-radius: 7px;
      padding: 0.5rem 0.6rem;
    }
    textarea {
      min-height: 6.5rem;
      resize: vertical;
    }
    .pair {
      display: grid;
      gap: 0.6rem;
      grid-template-columns: 1fr 1fr;
    }
    @media (max-width: 520px) {
      .pair {
        grid-template-columns: 1fr;
      }
    }
    button {
      font: inherit;
      font-weight: 600;
      cursor: pointer;
      border: 0;
      border-radius: 7px;
      padding: 0.45rem 0.9rem;
      background: var(--ic-brand);
      color: var(--ic-bg);
    }
    button[disabled] {
      opacity: 0.55;
      cursor: progress;
    }
    button.link {
      background: none;
      color: var(--ic-muted);
      padding: 0;
      font-weight: 500;
      text-decoration: underline;
      text-underline-offset: 2px;
    }
    .row {
      display: flex;
      justify-content: space-between;
      align-items: center;
      gap: 0.5rem;
    }
    ul {
      list-style: none;
      margin: 0;
      padding: 0;
      display: grid;
      gap: 1rem;
    }
    li {
      border-top: 1px solid var(--ic-line);
      padding-top: 1rem;
    }
    .children {
      margin-top: 0.8rem;
      padding-left: 0.9rem;
      border-left: 2px solid var(--ic-line);
      display: grid;
      gap: 0.9rem;
    }
    .head {
      display: flex;
      gap: 0.5rem;
      align-items: center;
      font-size: 0.85rem;
      color: var(--ic-muted);
    }
    .head b {
      color: var(--ic-fg);
      font-weight: 600;
    }
    .avatar {
      width: 22px;
      height: 22px;
      border-radius: 50%;
      object-fit: cover;
      background: var(--ic-line);
    }
    .body {
      margin: 0.35rem 0 0;
      overflow-wrap: anywhere;
    }
    .body :where(pre) {
      overflow-x: auto;
      background: color-mix(in oklab, var(--ic-line) 40%, transparent);
      padding: 0.6rem;
      border-radius: 6px;
    }
    .badges {
      display: flex;
      gap: 0.35rem;
      flex-wrap: wrap;
      margin-top: 0.3rem;
    }
    .badge {
      font-size: 0.72rem;
      border: 1px solid var(--ic-line);
      border-radius: 99px;
      padding: 0.05rem 0.45rem;
      color: var(--ic-muted);
    }
    .notice,
    .error {
      font-size: 0.85rem;
      border-radius: 7px;
      padding: 0.5rem 0.7rem;
      margin-bottom: 0.8rem;
    }
    .notice {
      background: color-mix(in oklab, var(--ic-ok) 12%, transparent);
      color: var(--ic-ok);
    }
    .error {
      background: color-mix(in oklab, var(--ic-bad) 10%, transparent);
      color: var(--ic-bad);
    }
    .muted {
      color: var(--ic-muted);
    }
    .reply {
      font-size: 0.85rem;
      color: var(--ic-muted);
      margin-bottom: 0.5rem;
    }
    footer {
      display: flex;
      justify-content: center;
      padding: 1rem 0 0;
    }
    a {
      color: inherit;
      text-underline-offset: 2px;
    }
  `;

  siteKey = "";
  apiBase = "";
  path = "";
  theme = "";
  private api: Api | null = null;
  private _view: View = blank();
  private turnstileToken: string | null = null;
  /** 一次草稿一把钥匙：重试同一段内容不会发重。 */
  private draftKey: string | null = null;

  override connectedCallback(): void {
    super.connectedCallback();
    this.api = new Api(this.apiBase || this.deriveBase(), this.siteKey);
    void this.load();
  }

  /** 没有 api-base 时，按脚本地址推断同源的 /api/v1（自托管最常见的情形）。 */
  private deriveBase(): string {
    return new URL("/api/v1", SELF_SRC || location.href).href;
  }

  private url(): string {
    return this.path || location.pathname + location.search;
  }

  private async load(cursor: string | null = null): Promise<void> {
    if (!this.api) return;
    this._view = { ...this._view, loading: true, error: null };
    try {
      const [meta, page] = await Promise.all([
        this._view.meta ?? this.api.meta(this.url()),
        this.api.page(this.url(), cursor),
      ]);
      this._view = {
        ...this._view,
        meta,
        comments: cursor ? [...this._view.comments, ...page.comments] : page.comments,
        next: page.next_cursor,
        total: page.total,
        loading: false,
      };
      this.maybeMountTurnstile();
    } catch (err) {
      this._view = { ...this._view, loading: false, error: describe(err) };
    }
  }

  private maybeMountTurnstile(): void {
    const meta = this._view.meta;
    if (!meta?.turnstile_site_key || meta.challenge_mode !== "always") return;
    if (document.querySelector("script[data-ic-turnstile]")) return;

    document.addEventListener("ic:turnstile", this.onTurnstile, { once: true });
    const script = document.createElement("script");
    script.src = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";
    script.async = true;
    script.dataset.icTurnstile = "1";
    script.onload = () => {
      // SAFETY: 这段只在 Turnstile 脚本 onload 后跑，globals 由它注入，TS 无从得知
      const w = window as unknown as {
        turnstile?: {
          render: (el: Element, opts: Record<string, unknown>) => void;
        };
      };
      const slot = this.renderRoot.querySelector(".turnstile");
      if (!w.turnstile || !slot) return;
      w.turnstile.render(slot, {
        sitekey: meta.turnstile_site_key,
        callback: (token: string) => {
          this.turnstileToken = token;
        },
        "expired-callback": () => {
          this.turnstileToken = null;
        },
        theme: window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light",
      });
    };
    document.head.append(script);
  }

  private onTurnstile = (event: Event): void => {
    this.turnstileToken = (event as CustomEvent<string>).detail;
  };

  private async submit(event: Event): Promise<void> {
    event.preventDefault();
    if (!this.api || this._view.sending) return;

    const body = this._view.body.trim();
    if (!body) {
      this._view = { ...this._view, error: "内容不能为空" };
      return;
    }
    const challenge = this._view.meta?.challenge_mode;
    if (challenge === "always" && !this.turnstileToken) {
      this._view = { ...this._view, error: "请先完成人机验证" };
      return;
    }

    this._view = { ...this._view, sending: true, error: null, notice: null };
    this.draftKey ??= crypto.randomUUID();
    try {
      const result = await this.api.post(this.url(), {
        body_md: body,
        parent_id: this._view.replyTo?.id ?? null,
        author_name: this._view.name.trim() || null,
        author_email: this._view.email.trim() || null,
        challenge_token: this.turnstileToken,
        idempotency_key: this.draftKey,
      });
      this._view = {
        ...this._view,
        sending: false,
        body: "",
        replyTo: null,
        notice: result.message ?? (result.created ? "已发布" : "已收到，审核后显示"),
      };
      this.turnstileToken = null;
      this.draftKey = null;
      if (result.created) await this.load();
    } catch (err) {
      const message =
        err instanceof ApiError && err.status === 429 ? "发得太快了，稍等一会儿" : describe(err);
      this._view = { ...this._view, sending: false, error: message };
    }
  }

  private reply(target: Comment): void {
    this._view = { ...this._view, replyTo: target, error: null };
    this.renderRoot.querySelector("textarea")?.focus();
  }

  private renderComment(
    comment: Comment,
    tree: Map<string | null, Comment[]>,
    depth: number,
  ): TemplateResult {
    const kids = tree.get(comment.id) ?? [];
    const badges = [];
    if (comment.status === "pending") badges.push("待审");
    if (comment.identity_unverified) badges.push("未验证");

    return html`<li>
      <div class="head">
        ${
          comment.author_avatar
            ? html`<img class="avatar" src=${comment.author_avatar} alt="" loading="lazy" />`
            : nothing
        }
        <b>${comment.author_name ?? "匿名"}</b>
        <time datetime=${comment.created_at}>${when(comment.created_at)}</time>
        <button class="link" type="button" @click=${() => this.reply(comment)}>回复</button>
      </div>
      <div class="body">${unsafe(comment.body_html)}</div>
      ${badges.length ? html`<div class="badges">${badges.map((b) => html`<span class="badge">${b}</span>`)}</div>` : nothing}
      ${
        kids.length && depth < 6
          ? html`<div class="children">
            ${repeat(
              kids,
              (c) => c.id,
              (c: Comment) => this.renderComment(c, tree, depth + 1),
            )}
          </div>`
          : nothing
      }
    </li>`;
  }

  /** 元信息都没拿到时，至少把访客引到服务端渲染的 embed 页去读和写。 */
  private failed(reason: string): TemplateResult {
    const page = this.url().replace(/^[^:]+:\/\/[^/]+/, "");
    const href = `${this.deriveOrigin()}/embed/${this.siteKey}${page}`;
    return html`
      <p class="error">${reason}</p>
      <p class="muted"><a href=${href} target="_blank" rel="noopener">在新页面读评论</a></p>
    `;
  }

  private deriveOrigin(): string {
    try {
      return new URL(this.apiBase || this.deriveBase()).origin;
    } catch {
      return location.origin;
    }
  }

  override render(): TemplateResult | TemplateResult[] {
    const v = this._view;
    const meta = v.meta;
    if (!meta && v.loading) return html`<p class="muted">加载评论…</p>`;
    if (!meta && v.error) return this.failed(v.error);

    const tree = byParent(v.comments);
    const roots = tree.get(null) ?? [];

    return html`
      <h2>
        ${v.total} 条评论
        ${meta?.require_approval ? html`<span class="count">（先审后发）</span>` : nothing}
      </h2>
      ${v.notice ? html`<p class="notice">${v.notice}</p>` : nothing}
      ${v.error ? html`<p class="error">${v.error}</p>` : nothing}

      <form @submit=${this.submit}>
        <fieldset>
          ${
            v.replyTo
              ? html`<div class="reply">
                回复 ${v.replyTo.author_name ?? "匿名"}
                <button class="link" type="button" @click=${() => (this._view = { ...v, replyTo: null })}>
                  取消
                </button>
              </div>`
              : nothing
          }
          <div class="pair">
            <input
              placeholder="昵称（可选）"
              autocomplete="nickname"
              .value=${live(v.name)}
              @input=${(e: InputEvent) =>
                (this._view = { ...this._view, name: (e.target as HTMLInputElement).value })}
            />
            <input
              type="email"
              placeholder="邮箱（可选，不公开）"
              autocomplete="email"
              .value=${live(v.email)}
              @input=${(e: InputEvent) =>
                (this._view = { ...this._view, email: (e.target as HTMLInputElement).value })}
            />
          </div>
          <textarea
            placeholder="说点什么… 支持 Markdown"
            maxlength=${meta?.max_body_bytes ?? 8192}
            .value=${live(v.body)}
            @input=${(e: InputEvent) =>
              (this._view = { ...this._view, body: (e.target as HTMLTextAreaElement).value })}
          ></textarea>
          ${meta?.challenge_mode === "always" ? html`<div class="turnstile"></div>` : nothing}
          <div class="row">
            <span class="muted">${v.body.length} 字</span>
            <button ?disabled=${v.sending || v.loading} type="submit">
              ${v.sending ? "发送中…" : "发表"}
            </button>
          </div>
        </fieldset>
      </form>

      <ul>
        ${repeat(
          roots,
          (c) => c.id,
          (c: Comment) => this.renderComment(c, tree, 0),
        )}
      </ul>
      ${
        roots.length === 0 && !v.loading
          ? html`<p class="muted">还没有评论，来说第一句。</p>`
          : nothing
      }
      ${
        v.next
          ? html`<footer>
            <button type="button" class="link" @click=${() => this.load(v.next)}>
              加载更多（${Math.min(PAGE_SIZE_HINT, v.total - v.comments.length)}）
            </button>
          </footer>`
          : nothing
      }
    `;
  }
}

/** 服务端的 HTML 已过白名单，这里直接插入。 */
function unsafe(markup: string) {
  const template = document.createElement("template");
  template.innerHTML = markup;
  return template.content.cloneNode(true);
}

function describe(err: unknown): string {
  if (err instanceof ApiError) {
    const first = Object.values(err.fields)[0];
    return first ?? err.message;
  }
  return err instanceof Error ? err.message : "请求失败";
}

function when(iso: string): string {
  const then = new Date(iso).getTime();
  const diff = Math.round((Date.now() - then) / 1000);
  if (Number.isNaN(diff)) return iso;
  if (diff < 60) return "刚刚";
  if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前`;
  if (diff < 86_400) return `${Math.floor(diff / 3600)} 小时前`;
  if (diff < 7 * 86_400) return `${Math.floor(diff / 86_400)} 天前`;
  return new Date(iso).toLocaleDateString("zh-CN");
}

// 自定义元素名必须带连字符。同一个 constructor 每个 registry 只能注册一次，
// 所以旧标签名走子类，避免 define 两次抛 NotSupportedError。
if (!customElements.get("ic-comment")) customElements.define("ic-comment", CommentWidget);
if (!customElements.get("icomment-widget")) {
  customElements.define("icomment-widget", class extends CommentWidget {});
}
