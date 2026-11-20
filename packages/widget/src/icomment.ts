// 入口：只负责注册元素，逻辑在 widget.ts。
import "./widget.js";

// 只贴 <script data-site-key> 也能挂上：脚本带 key 且页面里还没有组件时自己插一个。
const script = document.currentScript as HTMLScriptElement | null;
const key = script?.dataset.siteKey;
if (key && !document.querySelector("ic-comment")) {
  const el = document.createElement("ic-comment");
  el.setAttribute("site-key", key);
  if (script?.dataset.apiBase) el.setAttribute("api-base", script.dataset.apiBase);
  script?.after(el);
}

export type { Comment, Created, Page, SiteMeta } from "./api.js";
export { CommentWidget } from "./widget.js";
