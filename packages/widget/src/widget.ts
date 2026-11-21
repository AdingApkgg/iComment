import { css, html, LitElement, nothing, type TemplateResult } from "lit";
import { live } from "lit/directives/live.js";
import { repeat } from "lit/directives/repeat.js";
import { Api, ApiError, type Comment, type ReactionKind, type SiteMeta } from "./api.js";

const PAGE_SIZE_HINT = 40;
/** 与核心的 `MAX_PER_COMMENT` 对齐。 */
const MAX_IMAGES = 4;

/** 固定顺序，别跟服务端 enum 顺序脱节。 */
const REACTIONS: ReadonlyArray<readonly [ReactionKind, string, string]> = [
  ["up", "👍", "赞"],
  ["heart", "❤️", "喜欢"],
  ["haha", "😄", "好笑"],
  ["thinking", "🤔", "想想"],
  ["party", "🎉", "庆祝"],
];

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
  /** 正在提交反应，防连点 */
  reacting: string | null;
  /** 正在上传的图数 */
  uploading: number;
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
  reacting: null,
  uploading: 0,
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
    maxDepth: { type: Number, attribute: "max-depth" },
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
    :host([theme="light"]) {
      --ic-fg: #10131a;
      --ic-muted: #646d7c;
      --ic-line: #e2e6ee;
      --ic-bg: #fff;
      --ic-brand: #10131a;
      --ic-ok: #0b7a4b;
      --ic-bad: #b42318;
    }
    @media (prefers-reduced-motion: reduce) {
      :host {
        scroll-behavior: auto;
      }
    }
    /* 只给读屏用的标签，视觉上隐藏。 */
    .sr {
      position: absolute;
      width: 1px;
      height: 1px;
      margin: -1px;
      padding: 0;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
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
    .reacts {
      display: flex;
      gap: 0.3rem;
      flex-wrap: wrap;
      margin-top: 0.4rem;
    }
    .reacts button {
      font-size: 0.8rem;
      padding: 0.1rem 0.45rem;
      border-radius: 99px;
      border: 1px solid var(--ic-line);
      background: none;
      color: var(--ic-muted);
      min-height: 1.8rem;
    }
    .reacts button[aria-pressed="true"] {
      border-color: var(--ic-accent);
      color: var(--ic-accent);
      font-weight: 600;
    }
    .reacts button[disabled] {
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
  /** 折叠层级：缺省用站点设置，写了就以属性为准（服务端仍会挡）。 */
  maxDepth = 0;
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

  /** 点一下切换：还没投就投，投过就撤；同一条一次只能挂一个请求。 */
  private async react(comment: Comment, kind: ReactionKind): Promise<void> {
    if (this._view.reacting || !this.api) return;
    const on = !comment.reactions.mine.includes(kind);
    this._view = { ...this._view, reacting: `${comment.id}:${kind}` };
    try {
      const tally = await this.api.react(comment.id, kind, on);
      this._view = {
        ...this._view,
        reacting: null,
        comments: this._view.comments.map((c) =>
          c.id === comment.id ? { ...c, reactions: tally } : c,
        ),
      };
    } catch (err) {
      this._view = {
        ...this._view,
        reacting: null,
        error: err instanceof ApiError ? err.message : "表情没投上，再试一次",
      };
    }
  }

  private reactions(comment: Comment): TemplateResult | typeof nothing {
    const button = ([kind, emoji, label]: readonly [ReactionKind, string, string]) => {
      const n = comment.reactions.counts[kind] ?? 0;
      const mine = comment.reactions.mine.includes(kind);
      const busy = this._view.reacting === `${comment.id}:${kind}`;
      return html`<button
        type="button"
        aria-pressed=${mine ? "true" : "false"}
        aria-label=${`${label}，${n} 人`}
        ?disabled=${busy}
        @click=${() => this.react(comment, kind)}
      >${emoji}${n ? html`<span class="count">${n}</span>` : nothing}</button>`;
    };

    // 有人投过或自己投过的才挂出来；其余收进「＋」里，避免同一个表情出现两次
    const shown = REACTIONS.filter(
      ([kind]) =>
        (comment.reactions.counts[kind] ?? 0) > 0 || comment.reactions.mine.includes(kind),
    );
    const rest = REACTIONS.filter(([kind]) => !shown.some(([s]) => s === kind));

    const picker =
      rest.length > 0
        ? html`<details class="picker">
            <summary class="link" aria-label="加个表情">＋</summary>
            <div class="reacts">${rest.map(button)}</div>
          </details>`
        : nothing;

    if (shown.length === 0) return picker;
    return html`<div class="reacts">${shown.map(button)}${picker}</div>`;
  }

  /** 选图或粘贴时上传；完成前禁用发送，避免引用还没登记的附件。 */
  private reply(target: Comment): void {
    this._view = { ...this._view, replyTo: target, error: null };
    this.renderRoot.querySelector("textarea")?.focus();
  }

  /** 正文里已经引用的附图数。 */
  private images(): number {
    return this._view.body.split("ic-attachment:").length - 1;
  }

  /** 打开系统选图框；`input.hidden` 不占布局，选完即清空以允许重选同一张。 */
  private pick(): void {
    const input = this.renderRoot.querySelector<HTMLInputElement>("input[type=file]");
    if (!input) return;
    input.value = "";
    input.click();
  }

  /** 选图或粘贴时上传；完成前发送按钮保持可用但要等上传结束。 */
  private async attach(files: FileList | File[] | null): Promise<void> {
    if (!this.api || !files || files.length === 0) return;
    if (this._view.body.includes("ic-attachment:") && this.images() >= MAX_IMAGES) {
      this._view = { ...this._view, error: `最多 ${MAX_IMAGES} 张图` };
      return;
    }
    const room = Math.max(0, MAX_IMAGES - this.images());
    const picked = [...files].filter((f) => f.type.startsWith("image/")).slice(0, room);
    if (picked.length === 0) {
      this._view = { ...this._view, error: "只支持图片" };
      return;
    }
    this._view = { ...this._view, uploading: this._view.uploading + picked.length, error: null };
    for (const file of picked) {
      try {
        const stored = await this.api.upload(file);
        this._view = {
          ...this._view,
          uploading: this._view.uploading - 1,
          body: `${this._view.body.replace(/\s*$/, "")}${this._view.body.trim() ? "\n\n" : ""}${stored.markdown}`,
        };
      } catch (err) {
        this._view = {
          ...this._view,
          uploading: this._view.uploading - 1,
          error: err instanceof ApiError && err.status === 429 ? "图传得太快，稍等" : describe(err),
        };
      }
    }
  }

  private renderComment(
    comment: Comment,
    tree: Map<string | null, Comment[]>,
    depth: number,
    limit: number,
  ): TemplateResult {
    const kids = tree.get(comment.id) ?? [];
    const badges = [];
    if (comment.status === "pending") badges.push("待审");
    if (comment.identity_unverified) badges.push("未验证");

    return html`<li class="comment">
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
      ${this.reactions(comment)}
      ${badges.length ? html`<div class="badges">${badges.map((b) => html`<span class="badge">${b}</span>`)}</div>` : nothing}
      ${
        kids.length && depth < limit
          ? html`<div class="children">
            ${repeat(
              kids,
              (c) => c.id,
              (c: Comment) => this.renderComment(c, tree, depth + 1, limit),
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
    const limit = this.maxDepth || meta?.max_depth || 6;

    return html`
      <h2>
        ${v.total} 条评论
        ${meta?.require_approval ? html`<span class="count">（先审后发）</span>` : nothing}
      </h2>
      <div aria-live="polite">
        ${v.notice ? html`<p class="notice">${v.notice}</p>` : nothing}
        ${v.error ? html`<p class="error">${v.error}</p>` : nothing}
      </div>

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
            <label>
              <span class="sr">昵称</span>
              <input
                placeholder="昵称（可选）"
                autocomplete="nickname"
                .value=${live(v.name)}
                @input=${(e: InputEvent) =>
                  (this._view = { ...this._view, name: (e.target as HTMLInputElement).value })}
              />
            </label>
            <label>
              <span class="sr">邮箱（不公开）</span>
              <input
                type="email"
                placeholder="邮箱（可选，不公开）"
                autocomplete="email"
                .value=${live(v.email)}
                @input=${(e: InputEvent) =>
                  (this._view = { ...this._view, email: (e.target as HTMLInputElement).value })}
              />
            </label>
          </div>
          <label>
          <span class="sr">评论内容，支持 Markdown</span>
          <textarea
            placeholder="说点什么… 支持 Markdown"
            maxlength=${meta?.max_body_bytes ?? 8192}
            .value=${live(v.body)}
            @input=${(e: InputEvent) =>
              (this._view = { ...this._view, body: (e.target as HTMLTextAreaElement).value })}
            @paste=${(e: ClipboardEvent) => {
              const files = e.clipboardData?.files;
              if (files?.length) void this.attach(files);
            }}
          ></textarea>
          </label>
          <input
            type="file"
            accept="image/png,image/jpeg,image/webp,image/gif"
            multiple
            hidden
            @change=${(e: Event) => void this.attach((e.target as HTMLInputElement).files)}
          />
          ${meta?.challenge_mode === "always" ? html`<div class="turnstile"></div>` : nothing}
          <div class="row">
            <span class="muted">${v.body.length} 字</span>
            <button
              type="button"
              class="link"
              ?disabled=${v.uploading > 0 || v.sending}
              @click=${() => this.pick()}
            >
              ${v.uploading > 0 ? `上传中（${v.uploading}）…` : "插图"}
            </button>
            <button ?disabled=${v.sending || v.loading || v.uploading > 0} type="submit">
              ${v.sending ? "发送中…" : "发表"}
            </button>
          </div>
        </fieldset>
      </form>

      <ul ?aria-busy=${v.loading}>
        ${repeat(
          roots,
          (c) => c.id,
          (c: Comment) => this.renderComment(c, tree, 0, limit),
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

/**
 * 服务端的 HTML 已过 ammonia 白名单。这里用 DOMParser 解析再搬节点，
 * 不碰 innerHTML，解析失败时空白也比半截标签安全。
 */
function unsafe(markup: string) {
  const doc = new DOMParser().parseFromString(markup, "text/html");
  const fragment = document.createDocumentFragment();
  for (const node of doc.body.childNodes) {
    fragment.append(node.cloneNode(true));
  }
  return fragment;
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
