import {
  beforeAll,
  beforeEach,
  afterEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import type { ComponentType } from "react";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ show: async () => {} }),
}));
let Toolbox: ComponentType;
const keys = [1, 2].map((id) => ({
  id,
  name: `密钥${id}`,
  group: "DeepSeek",
  protocol: "openai-completions",
  usable: true,
  status: "active",
  quota: 0,
  quota_used: 0,
  expires_at: null,
}));
beforeAll(async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    value: {},
    configurable: true,
  });
  Toolbox = (await import("./Toolbox")).default;
});
beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation(async (command: string) => {
    if (command === "nftai_snapshot")
      return {
        user: {
          id: 1,
          username: "测试",
          email: "test@example.invalid",
          balance: 1,
        },
        keys,
      };
    if (command === "nftai_detect_tools")
      return [{ name: "dsh", version: null, installed_but_broken: false }];
    if (command === "nftai_models") return ["deepseek-chat"];
    if (command === "run_tool_lifecycle_action")
      throw new Error("安装失败，请重试");
    return undefined;
  });
});
afterEach(() => cleanup());
describe("guided configuration safety", () => {
  it("configures an installed DSH desktop without reinstalling its CLI", async () => {
    const normal = invoke.getMockImplementation()!;
    invoke.mockImplementation((command: string, args?: unknown) => {
      if (command === "nftai_detect_tools") return Promise.resolve([
        {name:"dsh", version:null, desktop_name:"DeepSeek Harness 桌面版", desktop_configurable:true},
        {name:"claude", version:null, desktop_name:"Claude 桌面版", desktop_configurable:false},
      ]);
      if (command === "nftai_apply") return Promise.resolve({providerId:"p",warnings:[],message:"已配置"});
      return normal(command, args);
    });
    render(<Toolbox />);
    await screen.findByText(/已安装 · DeepSeek Harness 桌面版/);
    expect(screen.getByText(/Claude Code 命令行未安装/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", {name:"配置模型"}));
    await waitFor(() => expect(screen.getByLabelText("使用模型")).toHaveValue("deepseek-chat"));
    fireEvent.click(screen.getByRole("button", {name:"应用配置"}));
    await waitFor(() => expect(invoke.mock.calls.some(([c]) => c === "nftai_apply")).toBe(true));
    expect(invoke.mock.calls.some(([c]) => c === "run_tool_lifecycle_action")).toBe(false);
  });
  it("shows pending detection instead of reporting every tool as missing", async () => {
    const normal = invoke.getMockImplementation()!;
    invoke.mockImplementation((command: string, args?: unknown) => command === "nftai_detect_tools" ? new Promise(() => {}) : normal(command, args));
    render(<Toolbox />);
    await screen.findByRole("heading", {name:"开始你的 AI 编程"});
    expect(screen.queryByText("尚未检测到安装")).not.toBeInTheDocument();
    expect(screen.getByRole("button", {name:"正在检测本机…"})).toBeDisabled();
    for (const button of screen.getAllByRole("button", {name:"安装并配置"})) expect(button).toBeDisabled();
  });
  it("explains an undeployed authorization service without opening a broken browser page", async () => {
    const normal = invoke.getMockImplementation()!;
    invoke.mockImplementation((command: string, args?: unknown) => {
      if (command === "nftai_snapshot") return Promise.reject(new Error("请先登录"));
      if (command === "nftai_login_start") return Promise.reject(new Error("网站尚未部署工具箱授权服务，请联系站长更新授权补丁。"));
      return normal(command, args);
    });
    render(<Toolbox />);
    const login = await screen.findByRole("button", { name: /在浏览器中登录 NFTAI/ });
    fireEvent.click(login);
    await screen.findByRole("alert");
    expect(screen.getByRole("alert")).toHaveTextContent("网站尚未部署工具箱授权服务");
    expect(invoke.mock.calls.some(([command]) => command === "open_external")).toBe(false);
    expect(login).toBeEnabled();
  });
  it("finishes login by polling when the native success event was missed", async () => {
    const normal = invoke.getMockImplementation()!;
    let snapshots = 0;
    invoke.mockImplementation((command: string, args?: unknown) => {
      if (command === "nftai_snapshot" && ++snapshots === 1) return Promise.reject(new Error("请先登录"));
      if (command === "nftai_login_start") return Promise.resolve("https://api.nftai.fun/toolbox/authorize?port=12345&state=test");
      if (command === "nftai_login_status") return Promise.resolve({ status: "authorized", error: null });
      return normal(command, args);
    });
    render(<Toolbox />);
    fireEvent.click(await screen.findByRole("button", { name: /在浏览器中登录 NFTAI/ }));
    await screen.findByRole("button", { name: /等待浏览器完成授权/ });
    await screen.findByRole("heading", { name: "开始你的 AI 编程" }, { timeout: 4500 });
    expect(snapshots).toBeGreaterThan(1);
    expect(screen.queryByText(/等待浏览器完成授权/)).not.toBeInTheDocument();
  });
  it("never writes configuration after installation fails", async () => {
    render(<Toolbox />);
    await screen.findByRole("heading", { name: "开始你的 AI 编程" });
    fireEvent.click(screen.getAllByRole("button", { name: "安装并配置" })[0]);
    await waitFor(() =>
      expect(screen.getByLabelText("使用模型")).toHaveValue("deepseek-chat"),
    );
    fireEvent.click(screen.getByRole("button", { name: "安装并应用配置" }));
    await screen.findAllByText(/安装失败，请重试/);
    expect(
      invoke.mock.calls.some(
        ([command]) => command === "run_tool_lifecycle_action",
      ),
    ).toBe(true);
    expect(
      invoke.mock.calls.some(([command]) => command === "nftai_apply"),
    ).toBe(false);
  });
  it("does not mix late model responses from a previously selected key", async () => {
    let resolveFirst!: (value: string[]) => void;
    const first = new Promise<string[]>((resolve) => {
      resolveFirst = resolve;
    });
    const normal = invoke.getMockImplementation()!;
    invoke.mockImplementation((command: string, args?: { keyId?: number }) => {
      if (command === "nftai_models")
        return args?.keyId === 1
          ? first
          : Promise.resolve(["second-key-model"]);
      return normal(command, args);
    });
    render(<Toolbox />);
    await screen.findByRole("heading", { name: "开始你的 AI 编程" });
    fireEvent.click(screen.getAllByRole("button", { name: "安装并配置" })[0]);
    fireEvent.change(screen.getByLabelText("API 密钥"), {
      target: { value: "2" },
    });
    await waitFor(() =>
      expect(screen.getByLabelText("使用模型")).toHaveValue("second-key-model"),
    );
    resolveFirst(["first-key-late-model"]);
    await waitFor(() =>
      expect(
        screen.queryByRole("option", { name: "first-key-late-model" }),
      ).not.toBeInTheDocument(),
    );
    expect(screen.getByLabelText("使用模型")).toHaveValue("second-key-model");
  });
});
