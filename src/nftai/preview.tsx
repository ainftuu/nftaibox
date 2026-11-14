// Development-only visual fixture. Not listed in the production Vite inputs.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
mockWindows("main");
mockIPC((command, args) => {
  if (command === "nftai_snapshot")
    return {
      user: {
        id: 0,
        username: "演示用户",
        email: "demo@example.invalid",
        balance: 128.5,
      },
      keys: [
        {
          id: 1,
          name: "DeepSeek 日常编程",
          group: "DeepSeek",
          protocol: "openai-completions",
          usable: true,
          status: "active",
          quota: 0,
          quota_used: 0,
          expires_at: null,
        },
        {
          id: 2,
          name: "Claude 编程专用",
          group: "Claude",
          protocol: "anthropic-messages",
          usable: true,
          status: "active",
          quota: 0,
          quota_used: 0,
          expires_at: null,
        },
        {
          id: 3,
          name: "Codex 编程专用",
          group: "OpenAI",
          protocol: "openai-responses",
          usable: true,
          status: "active",
          quota: 0,
          quota_used: 0,
          expires_at: null,
        },
      ],
    };
  if (command === "nftai_detect_tools")
    return [
      { name: "dsh", version: null, error: null, installed_but_broken: false },
      {
        name: "claude",
        version: "2.x",
        error: null,
        installed_but_broken: false,
      },
      {
        name: "codex",
        version: null,
        error: null,
        installed_but_broken: false,
      },
      {
        name: "opencode",
        version: "1.x",
        error: null,
        installed_but_broken: false,
      },
    ];
  if (command === "nftai_models")
    return (args as { keyId: number }).keyId === 1
      ? ["deepseek-chat", "deepseek-reasoner"]
      : (args as { keyId: number }).keyId === 2
        ? ["claude-sonnet-demo"]
        : ["codex-demo"];
  if (
    [
      "nftai_prepare_runtime",
      "nftai_apply",
      "run_tool_lifecycle_action",
      "nftai_launch",
      "open_external",
      "nftai_logout",
    ].includes(command)
  )
    throw new Error("这是界面预览，实际操作请使用桌面客户端。");
  return null;
});
void import("./main");
