# 组件与 widget

## widget（MIT）

`packages/widget`：Lit + Shadow DOM 的 `<icomment>`。

```html
<icomment site="ck_xxx" path="/posts/a" theme="auto" lang="zh"></icomment>
```

- 属性：`site-key`、`api-base`、`path`（缺省取 `location.pathname` 归一化）、`theme`(auto/light/dark)、`max-depth`（写了就盖过站点设置）。
- Shadow DOM 隔离：只用 CSS 自定义属性暴露主题（`--ic-accent`、`--ic-radius`、`--ic-font`、`--ic-size`），不漏全局样式。
- 服务端默认只给最小骨架；样式由 CDN 的 `v1` CSS 提供。
- 目标 ≤ 15KB gzip。首屏不拉评论，进入视口再取。

## 生命周期 / 降级

```
define → 校验属性 → 占位骨架 → 可见 → fetch thread.list
  ok        → 渲染
  空        → 渲染表单 + 空态
  失败/超时 → 渲染纯链接「在新页读评论」指向 /embed/<site>/<path>
JS 未加载   → 若页面作者同时放 <noscript> 链接则可见（snippet 里带）
```

`/embed/<site>/<path>` 是服务端渲染的独立评论页，无 JS 也能读和写（原生表单）。这层是所有 widget 故障的兜底，必须和 widget 同期存在。

## 父页面 → widget

`postMessage` 带 `origin` 白名单校验，用于：滚动到评论、通知未读数。widget 不读父页面的 cookie。

## 提交

- `POST` 带 `Idempotency-Key`（客户端生成 UUID），重试不重复。
- Turnstile token 在当前页面存活期内复用，失败重取。
- 服务端是真理：提交后重新拉一页，`pending` 的条目自己带「待审」徽章，不做乐观插入（避免半条假数据）。

## 无障碍

`aria-live="polite"` 播报结果；三个输入都有 `label`（视觉隐藏的 `.sr` 文案）；列表带 `aria-busy`；`prefers-reduced-motion` 时关平滑滚动；对比度 ≥ 4.5:1。Lighthouse accessibility 100 由 `deploy/demo/index.html` 跑出，S2 前每次改样式复跑。

## 暗色

`theme="auto"` 跟随 `prefers-color-scheme`，站主可锁定。Shadow DOM 内不引用父级变量，避免被站点主题污染。

## 依赖原则

零运行时依赖，不用框架、不用打包运行时。Lit 只作开发期与构建期，产物打平成原生自定义元素。widget 的 `package.json` 里没有除 Lit 之外的 dependency。
