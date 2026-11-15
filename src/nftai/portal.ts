export const TOOLS = [
  {
    id: "dsh",
    name: "DeepSeek Harness",
    mark: "DS",
    desc: "图形化编程助手 · 桌面版",
    tag: "推荐",
    protocols: ["openai-completions", "openai-responses", "anthropic-messages"],
  },
  {
    id: "claude",
    name: "Claude 桌面版",
    mark: "✳",
    desc: "图形化 AI 助手 · 桌面版",
    tag: "热门",
    protocols: ["anthropic-messages"],
  },
  {
    id: "codex",
    name: "Codex",
    mark: "›_",
    desc: "OpenAI 编程助手 · 桌面版",
    tag: "",
    protocols: ["openai-responses"],
  },
  {
    id: "opencode",
    name: "OpenCode",
    mark: "OC",
    desc: "开源编程助手 · 桌面版",
    tag: "开源",
    protocols: ["openai-completions", "openai-responses", "anthropic-messages"],
  },
] as const;
export type ToolId = (typeof TOOLS)[number]["id"];
export interface PortalKey {
  id: number;
  name: string;
  group: string | null;
  protocol: string;
  usable: boolean;
  status: string;
  quota: number;
  quota_used: number;
  expires_at: string | null;
}
export interface Snapshot {
  user: { id: number; username: string; email: string; balance: number };
  keys: PortalKey[];
}
export function compatibleKeys(tool: ToolId, keys: PortalKey[]) {
  const protocols: readonly string[] = TOOLS.find(
    (t) => t.id === tool,
  )!.protocols;
  return keys.filter((k) => k.usable && protocols.includes(k.protocol));
}
export function chooseModel(tool: ToolId, models: string[]) {
  const pattern =
    tool === "claude"
      ? /sonnet/i
      : tool === "codex"
        ? /codex/i
        : /deepseek.*(chat|flash)|deepseek-chat/i;
  return models.find((m) => pattern.test(m)) ?? models[0] ?? "";
}
export function friendlyError(error: unknown) {
  const message = String(error);
  if (/Node|npm|nodejs/i.test(message))
    return "缺少或无法使用 Node.js。请先安装 Node.js LTS，重新打开工具箱后重试。";
  if (/permission|EACCES|权限|denied/i.test(message))
    return "系统未允许安装或写入配置。请检查安装目录权限后重试。";
  if (/network|connect|timeout|网络|超时/i.test(message))
    return "连接暂时失败，请检查网络后重试。";
  return message.slice(0, 400);
}

export const DESKTOP_DOWNLOADS: Record<ToolId, string> = {
  dsh: "https://www.deepseek.com/en/download/",
  claude: "https://claude.com/download",
  codex: "https://chatgpt.com/download/",
  opencode: "https://opencode.ai/download",
};
