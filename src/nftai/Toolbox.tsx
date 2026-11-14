import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ArrowRight,
  Check,
  Coins,
  ChevronRight,
  Download,
  ExternalLink,
  KeyRound,
  LayoutDashboard,
  LogOut,
  Play,
  RefreshCw,
  Settings,
  ShieldCheck,
  Sparkles,
  X,
} from "lucide-react";
import {
  TOOLS,
  chooseModel,
  compatibleKeys,
  friendlyError,
  type Snapshot,
  type ToolId,
} from "./portal";
import "./toolbox.css";
import siteLogo from "../assets/nftai-logo.svg";

const native = "__TAURI_INTERNALS__" in window;
type Version = {
  name: string;
  version: string | null;
  error: string | null;
  installed_but_broken: boolean;
  desktop_name?: string | null;
  desktop_version?: string | null;
  desktop_configurable?: boolean;
};
const toolReady = (v?: Version) => !!v?.desktop_configurable || (!!v?.version && !v.installed_but_broken);
type ApplyResult = { providerId: string; warnings: string[]; message: string };
export default function Toolbox() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [tab, setTab] = useState("overview");
  const [versions, setVersions] = useState<Version[]>([]);
  const [detecting, setDetecting] = useState(false);
  const [detectionError, setDetectionError] = useState("");
  const detectingRef = useRef(false);
  const [busy, setBusy] = useState("");
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");
  const [loginPending, setLoginPending] = useState(false);
  const [selection, setSelection] = useState<ToolId | null>(null);
  const [keyId, setKeyId] = useState(0);
  const [models, setModels] = useState<string[]>([]);
  const [model, setModel] = useState("");
  const [modelBusy, setModelBusy] = useState(false);
  const [modelError, setModelError] = useState("");
  const [applied, setApplied] = useState<Record<string, ApplyResult>>({});
  const modelRequest = useRef(0);
  const actionLock = useRef(false);
  const open = (url: string) =>
    native
      ? invoke("open_external", { url })
      : Promise.resolve(window.open(url, "_blank", "noopener,noreferrer"));
  const fail = (e: unknown) => setError(friendlyError(e));
  const sync = useCallback(async () => {
    if (!native) return;
    const value = await invoke<Snapshot>("nftai_snapshot");
    setSnapshot(value);
    setLoginPending(false);
  }, []);
  const refreshTools = useCallback(async () => {
    if (!native || detectingRef.current) return;
    detectingRef.current = true;
    setDetecting(true);
    setDetectionError("");
    try {
      setVersions(await invoke<Version[]>("nftai_detect_tools"));
    } catch (e) {
      setDetectionError(friendlyError(e));
    } finally {
      detectingRef.current = false;
      setDetecting(false);
    }
  }, []);
  useEffect(() => {
    if (!native) return;
    void sync().catch(() => {});
    void refreshTools().catch(fail);
    const events = [
      listen("nftai-login", () => {
        void sync().catch(fail);
      }),
      listen<{ tools: string[]; stage: string; line?: string }>(
        "tool-lifecycle-progress",
        ({ payload }) => {
          if (payload.line) setNotice(payload.line.slice(-220));
        },
      ),
    ];
    // Native titlebar controls and dragging work on Windows and macOS.
    void getCurrentWindow()
      .show()
      .catch(() => {});
    return () => {
      events.forEach((p) => {
        void p.then((fn) => fn());
      });
    };
  }, [sync, refreshTools]);
  useEffect(() => {
    if (!loginPending) return;
    const started = Date.now();
    let active = true;
    let polling = false;
    const timer = window.setInterval(() => {
      if (!active || polling) return;
      if (Date.now() - started > 305000) {
        clearInterval(timer);
        setLoginPending(false);
        setError("登录等待已超时，请重新点击浏览器登录。");
        void invoke("nftai_login_cancel");
        return;
      }
      polling = true;
      void invoke<{ status: string; error: string | null }>("nftai_login_status")
        .then(async (value) => {
          if (!active) return;
          if (value.status === "authorized") {
            // Recover even if the native success event arrived before the UI listener.
            clearInterval(timer);
            setLoginPending(false);
            await sync();
          } else if (value.error || ["expired", "failed", "cancelled"].includes(value.status)) {
            clearInterval(timer);
            if (value.status !== "cancelled") setError(value.error || "授权已结束，请重新登录。");
            setLoginPending(false);
            void invoke("nftai_login_cancel");
          }
        })
        .catch((e) => { if (active) fail(e); })
        .finally(() => { polling = false; });
    }, 2000);
    return () => { active = false; clearInterval(timer); };
  }, [loginPending, sync]);
  useEffect(() => {
    const request = ++modelRequest.current;
    setModels([]);
    setModel("");
    setModelError("");
    if (!keyId || !selection || !native) {
      setModelBusy(false);
      return;
    }
    setModelBusy(true);
    void invoke<string[]>("nftai_models", { keyId })
      .then((list) => {
        if (request !== modelRequest.current) return;
        setModels(list);
        setModel(chooseModel(selection, list));
      })
      .catch((e) => {
        if (request === modelRequest.current) setModelError(friendlyError(e));
      })
      .finally(() => {
        if (request === modelRequest.current) setModelBusy(false);
      });
  }, [keyId, selection]);
  async function run(id: string, task: () => Promise<void>) {
    if (actionLock.current) return;
    actionLock.current = true;
    setBusy(id);
    setError("");
    setNotice("");
    try {
      await task();
    } catch (e) {
      fail(e);
    } finally {
      actionLock.current = false;
      setBusy("");
    }
  }
  function configure(tool: ToolId) {
    setSelection(tool);
    setKeyId(compatibleKeys(tool, snapshot?.keys ?? [])[0]?.id ?? 0);
  }
  async function installAndApply() {
    if (!selection || !keyId || !model || modelBusy) return;
    const tool = selection;
    await run(tool, async () => {
      const version = versions.find((v) => v.name === tool);
      if (!toolReady(version)) {
        await invoke("nftai_prepare_runtime", { tool });
        setNotice("正在安装，请留意系统弹出的安装确认…");
        await invoke("run_tool_lifecycle_action", {
          tools: [tool],
          action: "install",
        });
        // A successful installer exit alone does not prove that the tool is usable.
        const checked = await invoke<Version[]>("nftai_detect_tools");
        if (!toolReady(checked.find((v) => v.name === tool)))
          throw new Error("安装后仍无法运行工具，请重开工具箱后检查安装环境。");
        await refreshTools();
      }
      setNotice("正在验证模型并应用配置…");
      const result = await invoke<ApplyResult>("nftai_apply", {
        tool,
        keyId,
        model,
      });
      setApplied((previous) => ({ ...previous, [tool]: result }));
      setNotice([result.message, ...result.warnings].join(" "));
      setSelection(null);
    });
  }
  const titlebar = (
    <div className="nt-titlebar" data-tauri-drag-region>
      <span data-tauri-drag-region>NFTAI 工具箱</span>
      {native && (
        <div>
          <button
            aria-label="最小化"
            onClick={() => void getCurrentWindow().minimize()}
          >
            −
          </button>
          <button
            aria-label="最大化"
            onClick={() => void getCurrentWindow().toggleMaximize()}
          >
            □
          </button>
          <button
            aria-label="关闭"
            onClick={() => void getCurrentWindow().close()}
          >
            <X size={14} />
          </button>
        </div>
      )}
    </div>
  );
  if (!snapshot)
    return (
      <div className="nt-app">
        {titlebar}
        <main className="nt-login">
          <div className="nt-brand">
            <img className="nt-logo" src={siteLogo} alt="NFTAI" />
            <div>
              NFTAI 工具箱<small>你的 AI 编程，从这里开始</small>
            </div>
          </div>
          <span className="nt-eyebrow">WINDOWS & macOS</span>
          <h1>
            登录一次，
            <br />让 AI 工具准备就绪。
          </h1>
          <p>
            使用你的 NFTAI 中转站账号，自动同步密钥和模型。
            <br />
            选好工具，安装与配置交给我们。
          </p>
          <button
            className="nt-primary nt-login-button"
            disabled={!!busy || loginPending || !native}
            onClick={() =>
              void run("login", async () => {
                const url = await invoke<string>("nftai_login_start");
                setLoginPending(true);
                try {
                  await open(url);
                } catch (error) {
                  setLoginPending(false);
                  await invoke("nftai_login_cancel");
                  throw error;
                }
              })
            }
          >
            {loginPending ? "等待浏览器完成授权…" : "在浏览器中登录 NFTAI"}
            <ArrowRight size={18} />
          </button>
          {loginPending && (
            <button
              className="nt-link"
              onClick={() =>
                void run("cancel-login", async () => {
                  await invoke("nftai_login_cancel");
                  setLoginPending(false);
                })
              }
            >
              取消本次登录
            </button>
          )}
          <div className="nt-login-help">
            <ShieldCheck size={15} /> 密码只在 NFTAI
            网站输入，登录凭据保存在系统凭据库。
          </div>
          {!native && (
            <p className="nt-preview">
              网页预览：账号登录、工具安装与配置需要在桌面客户端中使用。
            </p>
          )}
          {error && (
            <div role="alert" className="nt-error">
              {error}
            </div>
          )}
          <button
            className="nt-link"
            onClick={() =>
              void open("https://api.nftai.fun/register").catch(fail)
            }
          >
            还没有账号？前往注册 <ExternalLink size={13} />
          </button>
        </main>
      </div>
    );
  const keys = snapshot.keys;
  return (
    <div className="nt-app">
      {titlebar}
      <div className="nt-layout">
        <aside className="nt-sidebar">
          <div className="nt-brand">
            <img className="nt-logo" src={siteLogo} alt="NFTAI" />
            <div>
              NFTAI<small>AI 编程工具箱</small>
            </div>
          </div>
          <nav>
            {[
              { id: "overview", label: "概览", icon: LayoutDashboard },
              { id: "keys", label: "我的密钥", icon: KeyRound },
              { id: "settings", label: "设置", icon: Settings },
            ].map((item) => (
              <button
                key={item.id}
                className={tab === item.id ? "active" : ""}
                onClick={() => setTab(item.id)}
              >
                <item.icon size={18} />
                {item.label}
              </button>
            ))}
          </nav>
          <div className="nt-sidebar-bottom">
            <span className="nt-avatar">
              {(snapshot.user.username || "N").slice(0, 1).toUpperCase()}
            </span>
            <div>
              <b>{snapshot.user.username || "NFTAI 用户"}</b>
              <small>已连接 NFTAI</small>
            </div>
          </div>
        </aside>
        <main className="nt-content">
          <header className="nt-heading">
            <div>
              <span className="nt-eyebrow">YOUR AI WORKSPACE</span>
              <h1>
                {tab === "overview"
                  ? "开始你的 AI 编程"
                  : tab === "keys"
                    ? "我的 API 密钥"
                    : "工具箱设置"}
              </h1>
            </div>
            <button
              className="nt-secondary"
              disabled={!!busy}
              onClick={() =>
                void run("sync", async () => {
                  await sync();
                  await refreshTools();
                  setNotice("账户、密钥与工具状态已同步。");
                })
              }
            >
              <RefreshCw
                size={15}
                className={busy === "sync" ? "nt-spin" : ""}
              />
              同步
            </button>
          </header>
          {error && (
            <div role="alert" className="nt-error">
              {error}
            </div>
          )}
          {notice && (
            <div role="status" className="nt-notice">
              {notice}
            </div>
          )}
          {tab === "overview" && (
            <>
              <section className="nt-stats">
                <article>
                  <Coins className="nt-stat-icon" aria-hidden="true" />
                  <div>
                    <small>账户余额 · 以网站计费单位为准</small>
                    <strong>
                      {Number(snapshot.user.balance || 0).toFixed(2)}
                    </strong>
                  </div>
                  <button
                    className="nt-link"
                    onClick={() =>
                      void open("https://api.nftai.fun/wallet").catch(fail)
                    }
                  >
                    前往网站 <ChevronRight size={15} />
                  </button>
                </article>
                <article>
                  <KeyRound className="nt-stat-icon" aria-hidden="true" />
                  <div>
                    <small>可用 API 密钥</small>
                    <strong>
                      {keys.filter((k) => k.usable).length}
                      <em> / {keys.length} 个</em>
                    </strong>
                  </div>
                  <button className="nt-link" onClick={() => setTab("keys")}>
                    查看密钥 <ChevronRight size={15} />
                  </button>
                </article>
              </section>
              <section className="nt-guide">
                <span className="nt-guide-symbol">
                  <Sparkles size={23} />
                </span>
                <div>
                  <b>选一个工具，剩下的交给工具箱。</b>
                  <p>自动匹配可用密钥和模型，帮你完成安装与配置。</p>
                </div>
                <span className="nt-guide-steps">
                  ① 选择工具 <ChevronRight size={13} /> ② 一键准备{" "}
                  <ChevronRight size={13} /> ③ 开始使用
                </span>
              </section>
              <div className="nt-section-title">
                <h2>AI 编程工具</h2>
                <button className="nt-secondary" disabled={detecting} onClick={() => void refreshTools()}>
                  <RefreshCw size={14} className={detecting ? "nt-spin" : ""} />
                  {detecting ? "正在检测本机…" : "重新检测"}
                </button>
              </div>
              {detectionError && <p role="alert">本机检测失败：{detectionError}，请点“重新检测”。</p>}
              <section className="nt-tools">
                {TOOLS.map((tool) => {
                  const v = versions.find((v) => v.name === tool.id);
                  const configured = applied[tool.id];
                  const count = compatibleKeys(tool.id, keys).length;
                  return (
                    <article key={tool.id} className="nt-tool">
                      <span className={`nt-tool-mark nt-${tool.id}`}>
                        {tool.mark}
                      </span>
                      <div className="nt-tool-info">
                        <h3>
                          {tool.name}
                          {tool.tag && (
                            <span className="nt-tag">{tool.tag}</span>
                          )}
                        </h3>
                        <p>{tool.desc}</p>
                        <small className={configured ? "nt-ready" : ""}>
                          {configured ? (
                            <>
                              <Check size={12} />
                              本次已配置
                            </>
                          ) : !v ? (
                            detecting ? "正在检测本机…" : "尚未完成检测"
                          ) : v.desktop_configurable ? (
                            `已安装 · ${v.desktop_name}${v.desktop_version ? ` ${v.desktop_version}` : ""}`
                          ) : v?.installed_but_broken ? (
                            "已安装，运行异常"
                          ) : v?.version ? (
                            `已安装命令行版 · ${v.version}`
                          ) : v.desktop_name ? (
                            `已安装 ${v.desktop_name}；Claude Code 命令行未安装`
                          ) : (
                            "尚未检测到安装"
                          )}
                          <span>·</span>
                          {count ? `${count} 个适配密钥` : "暂无适配密钥"}
                        </small>
                      </div>
                      <div className="nt-tool-actions">
                        {toolReady(v) && (
                          <button
                            className="nt-secondary"
                            disabled={!!busy}
                            onClick={() =>
                              void run("launch", async () => {
                                await invoke("nftai_launch", { tool: tool.id });
                              })
                            }
                          >
                            <Play size={14} />
                            启动
                          </button>
                        )}
                        <button
                          className="nt-primary"
                          disabled={!!busy || detecting || !v}
                          onClick={() => configure(tool.id)}
                        >
                          {busy === tool.id ? (
                            <RefreshCw className="nt-spin" size={15} />
                          ) : (
                            <Download size={15} />
                          )}
                          {toolReady(v) ? "配置模型" : v?.desktop_name ? "安装命令行并配置" : "安装并配置"}
                        </button>
                      </div>
                    </article>
                  );
                })}
              </section>
              <p className="nt-footnote">
                安装过程可能需要系统确认。已在运行的 AI
                工具，请在配置后新开会话或重启。
              </p>
            </>
          )}
          {tab === "keys" && (
            <>
              <p className="nt-subtitle">
                从 NFTAI 自动同步。这里只显示密钥名称与状态，不展示完整密钥。
              </p>
              <div className="nt-key-list">
                {keys.map((key) => (
                  <article key={key.id}>
                    <KeyRound size={20} />
                    <div>
                      <b>{key.name}</b>
                      <p>
                        {key.group || "未绑定分组"} ·{" "}
                        {key.protocol === "unknown"
                          ? "该分组暂不支持自动配置"
                          : key.protocol}
                      </p>
                    </div>
                    <span
                      className={key.usable ? "nt-key-active" : "nt-key-muted"}
                    >
                      {key.usable ? "可用" : key.status}
                    </span>
                  </article>
                ))}
                {!keys.length && (
                  <div className="nt-empty">
                    还没有 API 密钥，请先在网站创建密钥，然后点击同步。
                  </div>
                )}
              </div>
              <button
                className="nt-secondary"
                onClick={() =>
                  void open("https://api.nftai.fun/keys").catch(fail)
                }
              >
                在网站管理密钥 <ExternalLink size={14} />
              </button>
            </>
          )}
          {tab === "settings" && (
            <section className="nt-settings">
              <h2>你的账号</h2>
              <p>{snapshot.user.email}</p>
              <p>服务地址：api.nftai.fun</p>
              <p>版本：0.1.6 · Windows / macOS</p>
              <button
                className="nt-secondary"
                disabled={!!busy}
                onClick={() =>
                  void run("logout", async () => {
                    await invoke("nftai_logout");
                    setSnapshot(null);
                    setApplied({});
                    setNotice("");
                  })
                }
              >
                <LogOut size={15} />
                退出登录
              </button>
              <hr />
              <h2>安装环境</h2>
              <p>若提示缺少 Node.js，请安装 Node.js LTS 后重新打开工具箱。</p>
              <button
                className="nt-secondary"
                onClick={() =>
                  void open("https://nodejs.org/zh-cn/download").catch(fail)
                }
              >
                下载 Node.js <ExternalLink size={14} />
              </button>
            </section>
          )}
        </main>
      </div>
      {selection && (
        <div className="nt-overlay">
          <section
            className="nt-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="nt-dialog-title"
          >
            <button
              aria-label="关闭"
              className="nt-close"
              disabled={!!busy}
              onClick={() => setSelection(null)}
            >
              <X size={19} />
            </button>
            <span className="nt-eyebrow">READY IN A FEW STEPS</span>
            <h2 id="nt-dialog-title">
              准备 {TOOLS.find((t) => t.id === selection)?.name}
            </h2>
            <p>{toolReady(versions.find((v) => v.name === selection)) ? "已检测到安装，确认后直接应用配置。" : "已为你筛选适配密钥。确认后自动安装命令行工具并应用配置。"}</p>
            <label>
              API 密钥
              <select
                disabled={!!busy}
                value={keyId}
                onChange={(e) => setKeyId(Number(e.target.value))}
              >
                <option value={0}>请选择密钥</option>
                {compatibleKeys(selection, keys).map((key) => (
                  <option key={key.id} value={key.id}>
                    {key.name} · {key.group || "默认分组"}
                  </option>
                ))}
              </select>
            </label>
            {!compatibleKeys(selection, keys).length && (
              <div className="nt-error">
                暂无适配密钥。Claude Code 需要 Anthropic 协议分组，Codex 需要
                Responses 协议分组。请先在网站创建对应密钥，再同步。
              </div>
            )}
            <label>
              使用模型
              <select
                disabled={modelBusy || !!busy}
                value={model}
                onChange={(e) => setModel(e.target.value)}
              >
                <option value="">
                  {modelBusy ? "正在获取可用模型…" : "请选择模型"}
                </option>
                {models.map((m) => (
                  <option key={m} value={m}>
                    {m}
                  </option>
                ))}
              </select>
            </label>
            {modelError && (
              <div role="alert" className="nt-error">
                {modelError}
                <button
                  className="nt-link"
                  onClick={() => {
                    const current = keyId;
                    setKeyId(0);
                    window.setTimeout(() => setKeyId(current), 0);
                  }}
                >
                  重试
                </button>
              </div>
            )}
            {selection === "dsh" && (
              <p className="nt-footnote">
                首次导入后，需要在 DSH 的模型选择器中选择 NFTAI 下的模型。
              </p>
            )}
            <div className="nt-config-summary">
              <ShieldCheck size={16} />
              先验证模型，再应用配置。沿用 CC Switch 的配置保护机制。
            </div>
            {error && (
              <div role="alert" className="nt-error">
                {error}
              </div>
            )}
            {notice && (
              <div role="status" className="nt-notice">
                {notice}
              </div>
            )}
            <button
              className="nt-primary nt-full"
              disabled={!!busy || !keyId || !model || modelBusy}
              onClick={() => void installAndApply()}
            >
              {busy ? (
                <>
                  <RefreshCw size={16} className="nt-spin" />
                  正在准备，请稍候…
                </>
              ) : (
                <>
                  {toolReady(versions.find((v) => v.name === selection)) ? "应用配置" : "安装并应用配置"}
                  <ArrowRight size={16} />
                </>
              )}
            </button>
          </section>
        </div>
      )}
    </div>
  );
}
