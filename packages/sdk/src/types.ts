/** 线上接口的形状。跟核心的 handler 一一对应。 */

export type Site = {
  id: string;
  origin: string;
  name: string | null;
  require_approval: boolean;
  challenge_mode: string;
  /** 公开的站点 key，嵌页时用；不是凭据 */
  site_key: string;
  created_at: string;
};

export type SiteCreated = Site & {
  site_token: string;
  embed_snippet: string;
};

export type Comment = {
  id: string;
  thread_id: string;
  parent_id: string | null;
  body_md: string;
  body_html: string;
  author_name: string | null;
  author_avatar: string | null;
  author_email: string | null;
  author_kind: string;
  status: string;
  identity_unverified: boolean;
  created_at: string;
};

export type CommentPage = {
  comments: Comment[];
  next_cursor: string | null;
  total: number;
};

export type Thread = {
  id: string;
  site_id: string;
  path: string;
  comment_count: number;
  last_at: string | null;
};

export type Stats = {
  comments_total: number;
  comments_pending: number;
  comments_spam: number;
  comments_week: number;
  threads: number;
};

export type Me = {
  account_id: string;
  site_id: string | null;
  scope: "account" | "site";
};

export type DeviceStart = {
  device_code: string;
  user_code: string;
  verification_uri: string;
  verification_uri_complete: string;
  expires_in: number;
  interval: number;
};

export type TokenIssued = {
  token: string;
  account_id: string;
};

export type ModerateAction = "approve" | "hide" | "spam" | "delete";

export type VerifyFound = "script" | "element" | "site_key" | "path";

export type VerifyResult = {
  url: string;
  http_status: number | null;
} & (
  | { status: "ok"; found: VerifyFound[] }
  | { status: "partial"; missing: VerifyFound[]; hint: string }
  | { status: "failed"; reason: string; hint: string }
);
