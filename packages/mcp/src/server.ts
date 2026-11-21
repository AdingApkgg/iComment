/** MCP 工具集：把 agent 建站/管评的流程封成工具。 */

import { Client, ICommentError } from "@icomment/sdk";
import { McpServer } from "@modelcontextprotocol/server";
import { z } from "zod";
import { apiBase, loadToken, saveToken } from "./config.js";

const siteTokenSchema = z
  .string()
  .optional()
  .describe("覆盖默认凭据的站点 token；不填则用登录时保存的 token");

function text(value: unknown) {
  return { content: [{ type: "text" as const, text: JSON.stringify(value, null, 2) }] };
}

function client(token?: string): Client {
  const resolved = token ?? loadToken();
  if (!resolved) {
    throw new Error("还没有凭据：先跑 `icomment-mcp login`，或设置 IC_TOKEN 环境变量");
  }
  return new Client({ base: apiBase(), token: resolved });
}

function wrap<A>(fn: (args: A) => Promise<unknown>) {
  return async (args: A) => {
    try {
      return text(await fn(args));
    } catch (err) {
      if (err instanceof ICommentError) {
        return {
          isError: true,
          content: [
            {
              type: "text" as const,
              text: `${err.code}: ${err.message}${Object.keys(err.problem.fields).length ? `\n${JSON.stringify(err.problem.fields, null, 2)}` : ""}`,
            },
          ],
        };
      }
      throw err;
    }
  };
}

export function buildServer(): McpServer {
  const server = new McpServer({ name: "icomment", version: "0.1.0" });

  server.registerTool(
    "login",
    {
      title: "登录",
      description: "启动 device flow 打印一次性登录链接，用户批准后保存 token",
      inputSchema: z.object({}),
    },
    wrap(async () => {
      const anon = new Client({ base: apiBase() });
      const flow = await anon.startDeviceFlow();
      const deadline = Date.now() + flow.expires_in * 1000;
      let issued: { token: string; account_id: string } | null = null;
      while (!issued && Date.now() < deadline) {
        await new Promise((r) => setTimeout(r, flow.interval * 1000));
        issued = await anon.pollDeviceFlow(flow.device_code);
      }
      if (!issued) return { status: "expired", verification_uri: flow.verification_uri };
      saveToken(issued.token);
      return { status: "ok", account_id: issued.account_id };
    }),
  );

  server.registerTool(
    "start_login",
    {
      title: "开始登录（不阻塞）",
      description: "只返回验证链接与 device_code，由调用方稍后用 finish_login 换 token",
      inputSchema: z.object({}),
    },
    wrap(async () => new Client({ base: apiBase() }).startDeviceFlow()),
  );

  server.registerTool(
    "finish_login",
    {
      title: "完成登录",
      description: "用 start_login 拿到的 device_code 换 token 并保存",
      inputSchema: z.object({ device_code: z.string() }),
    },
    wrap(async ({ device_code }) => {
      const issued = await new Client({ base: apiBase() }).pollDeviceFlow(device_code);
      if (!issued) return { status: "pending" };
      saveToken(issued.token);
      return { status: "ok", account_id: issued.account_id };
    }),
  );

  server.registerTool(
    "whoami",
    {
      title: "当前身份",
      description: "查看当前 token 绑定的账号与站点",
      inputSchema: z.object({ token: siteTokenSchema }),
    },
    wrap(async ({ token }) => client(token).me()),
  );

  server.registerTool(
    "list_sites",
    {
      title: "列出站点",
      description: "列出当前账号的站点",
      inputSchema: z.object({ token: siteTokenSchema }),
    },
    wrap(async ({ token }) => client(token).listSites()),
  );

  server.registerTool(
    "claim_site",
    {
      title: "申请域名归属校验",
      description:
        "生产环境建站第一步：拿到要放到域名根目录的 key 文件地址与内容。发布后把 claim_key 传给 create_site",
      inputSchema: z.object({
        origin: z.string().describe("站点源，如 blog.example.com 或 https://blog.example.com"),
        token: siteTokenSchema,
      }),
    },
    wrap(async ({ origin, token }) => client(token).startClaim(origin)),
  );

  server.registerTool(
    "create_site",
    {
      title: "创建站点",
      description:
        "注册一个新站点，返回 site_key、站点 token 和可直接粘贴的嵌入片段；贴完后用 verify_install 验收。生产环境要先 claim_site 并把 key 文件发布到域名根下，再带上 claim_key",
      inputSchema: z.object({
        origin: z.string().describe("站点源，如 blog.example.com 或 https://blog.example.com"),
        name: z.string().optional(),
        blocked_domains: z.array(z.string()).optional(),
        claim_key: z
          .string()
          .optional()
          .describe("claim_site 返回的 claim_key；服务端开启域名校验时必填"),
        token: siteTokenSchema,
      }),
    },
    wrap(async ({ origin, name, blocked_domains, claim_key, token }) =>
      client(token).createSite({ origin, name, blocked_domains, claim_key }),
    ),
  );

  server.registerTool(
    "update_site",
    {
      title: "修改站点设置",
      description:
        "改站点名、是否先审后发、人机验证策略、域名黑名单（命中即进垃圾箱，传空数组清空）",
      inputSchema: z.object({
        site_id: z.string(),
        name: z.string().optional(),
        require_approval: z.boolean().optional(),
        challenge_mode: z.enum(["off", "always", "suspicious"]).optional(),
        blocked_domains: z.array(z.string()).optional(),
        token: siteTokenSchema,
      }),
    },
    wrap(async ({ site_id, token, ...rest }) => client(token).updateSite(site_id, rest)),
  );

  server.registerTool(
    "sign_identity",
    {
      title: "签透传身份",
      description:
        "给宿主站已登录用户签一段 5 分钟有效的 X-IC-Identity，前端带给评论接口即可免人机验证、免待审。秘钥不出服务端。",
      inputSchema: z.object({
        site_id: z.string(),
        uid: z.string().describe("宿主站自己的用户 id，只在站内有效"),
        name: z.string().optional(),
        token: siteTokenSchema,
      }),
    },
    wrap(async ({ site_id, token, uid, name }) => client(token).signIdentity(site_id, uid, name)),
  );

  server.registerTool(
    "verify_install",
    {
      title: "验收安装",
      description:
        "让服务端去抓页面，检查 script、ic-comment 元素、site-key、path 四要素；缺项会给出可行动 hint",
      inputSchema: z.object({
        site_id: z.string(),
        url: z.string().describe("部署后的完整页面地址，必须公网可访问"),
        token: siteTokenSchema,
      }),
    },
    wrap(async ({ site_id, url, token }) => client(token).verifyInstall(site_id, url)),
  );

  server.registerTool(
    "list_comments",
    {
      title: "列出评论",
      description: "按状态翻评论；status 可取 pending/visible/hidden/spam/deleted/all",
      inputSchema: z.object({
        site_id: z.string(),
        status: z.string().optional(),
        cursor: z.string().optional(),
        limit: z.number().int().min(1).max(100).optional(),
        token: siteTokenSchema,
      }),
    },
    wrap(async ({ site_id, token, ...rest }) => client(token).listComments(site_id, rest)),
  );

  server.registerTool(
    "create_comment",
    {
      title: "以站主身份发评论",
      description: "直接发布（跳过审核），常用于博主回复",
      inputSchema: z.object({
        site_id: z.string(),
        path: z.string().describe("文章路径，如 /posts/hello"),
        body_md: z.string(),
        parent_id: z.string().optional(),
        token: siteTokenSchema,
      }),
    },
    wrap(async ({ site_id, token, ...rest }) => client(token).createComment(site_id, rest)),
  );

  server.registerTool(
    "moderate_comment",
    {
      title: "审核评论",
      description: "对单条评论执行 approve/hide/spam/delete",
      inputSchema: z.object({
        site_id: z.string(),
        comment_id: z.string(),
        action: z.enum(["approve", "hide", "spam", "delete"]),
        token: siteTokenSchema,
      }),
    },
    wrap(async ({ site_id, comment_id, action, token }) =>
      client(token).moderate(site_id, comment_id, action),
    ),
  );

  server.registerTool(
    "list_threads",
    {
      title: "列出文章",
      description: "列出有评论的文章",
      inputSchema: z.object({ site_id: z.string(), token: siteTokenSchema }),
    },
    wrap(async ({ site_id, token }) => client(token).listThreads(site_id)),
  );

  server.registerTool(
    "audit_log",
    {
      title: "审核留痕",
      description: "最近的放行、隐藏、删除记录。传 comment_id 只看那一条的历史",
      inputSchema: z.object({
        site_id: z.string(),
        comment_id: z.string().optional(),
        token: siteTokenSchema,
      }),
    },
    wrap(async ({ site_id, comment_id, token }) => client(token).auditLog(site_id, comment_id)),
  );

  server.registerTool(
    "upload_attachment",
    {
      title: "上传附图",
      description:
        "传一张 PNG/JPEG/WebP/GIF（≤4MiB），把返回的 markdown 插进 body_md；每条评论最多 4 张",
      inputSchema: z.object({
        site_id: z.string(),
        /** 图片字节的 base64（不带 data: 前缀） */
        data_base64: z.string(),
        filename: z.string().optional(),
        token: siteTokenSchema,
      }),
    },
    wrap(async ({ site_id, data_base64, filename, token }) => {
      const bytes = Buffer.from(data_base64, "base64");
      return client(token).upload(site_id, new Blob([bytes]), filename ?? "upload.png");
    }),
  );

  server.registerTool(
    "site_stats",
    {
      title: "站点统计",
      description: "评论总数、待审、垃圾、本周新增",
      inputSchema: z.object({ site_id: z.string(), token: siteTokenSchema }),
    },
    wrap(async ({ site_id, token }) => client(token).stats(site_id)),
  );

  return server;
}
