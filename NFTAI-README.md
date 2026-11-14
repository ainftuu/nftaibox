# NFTAI 工具箱 · Windows / macOS 第一版

0.1.6：移除登录页底部开源说明、设置页开源致谢标题及正文，删除对应无用样式。LICENSE 与 UPSTREAM.json 作为安装资源随 Windows/macOS 包分发，源码中继续保留版权及许可证文件。

0.1.5：修复 0.1.4 的 303 跳转被 Chromium 表单 CSP 拦截的问题。成功回调返回 200 HTML，完成表单导航后通过零秒 meta refresh 前往官网；保留手动首页链接、no-store/no-referrer、严格回调校验，失败不导航。受限 form-action 的浏览器测试已实际到达 https://nftai.fun/；未使用真实凭据。

0.1.4：本机授权完成且登录凭据保存成功后，返回 HTTP 303，浏览器立即以 GET 打开 https://nftai.fun/，不再停留本机成功页。响应禁止缓存和发送来源，登录失败不跳转；不在首页 URL 中携带登录凭据。

0.1.3 界面调整：账户余额改用钱币图标，余额及密钥图标统一墨黑 #121417 背景、荧光绿 #B8EE3C 描边；删除设置中的“查看上游项目”按钮，保留开源致谢与许可文件。

0.1.2 修复：本机安装检测不再调用联网版本更新接口；Windows 识别 DSH 桌面安装和 Codex/Claude 的 MSIX 安装记录，macOS 识别系统及用户 Applications 目录。DSH/Codex 桌面版直接配置和启动，避免重复安装 CLI；Claude Desktop 不冒充 Claude Code，明确说明命令行是否已安装。新增检测中状态、检测失败说明及重新检测按钮。

0.1.1 修复：登录前核对网站授权协议；网站未部署接口时立即说明原因；增加原生登录状态轮询，避免丢失成功事件后一直等待；界面使用与现网一致的图形 Logo、瓷白底、墨色胶囊按钮及紫色强调。

基于 CC Switch 的独立品牌客户端。上游版本、提交与许可见 `UPSTREAM.json` 和 `LICENSE`。

## 已实现的源码功能

- 简体中文登录页、账户余额、密钥列表、四款工具列表与配置向导。
- 浏览器登录 + 随机本地回调端口 + 一次性随机 state；回调仅接受 `https://api.nftai.fun` 来源。
- 网站签发独立桌面登录会话，不复制浏览器 refresh token；登录凭据使用 Windows 凭据库 / macOS Keychain。
- 自动刷新会话、分页同步密钥；完整密钥只在原生后端使用，不返回渲染界面。
- 按分组协议筛选密钥，按实际 `/v1/models` 列表选择模型；不把模型发现成功标成推理测试成功。
- 复用上游 DSH / Claude Code / Codex / OpenCode 配置适配、安装进度和版本探测；安装后重新检测工具是否可运行。
- 自己的数据目录 `~/.nftai-toolbox` 与独立应用 ID；目标工具的 live 配置仍是原生工具目录。
- 原版更新地址已经移除，不会用 CC Switch 官方更新包覆盖此定制版。

## 开发

要求 Node.js 22、pnpm 10.12.3、Rust stable；Windows 还需 MSVC C++ 构建工具，macOS 需要 Xcode Command Line Tools。

```sh
corepack pnpm install --frozen-lockfile
corepack pnpm typecheck
corepack pnpm exec vitest run src/nftai
corepack pnpm build:renderer
# RUSTUP_TOOLCHAIN=stable（Windows PowerShell 用 $env:RUSTUP_TOOLCHAIN='stable'）
corepack pnpm tauri dev
```

## 网站授权接入：必须先部署

仅安装客户端，不能凭空获得现网登录回调功能。将 `portal-integration` 中补丁应用到当前部署使用的 Sub2API 源码：

```sh
node portal-integration/apply.cjs /absolute/path/to/sub2api
```

脚本先检查源码锚点、备份改动前文件，再增加：

1. 受现有登录守卫保护的 `/toolbox/authorize` Vue 页面。
2. 与 `/auth/me` 相同 JWT / 面板限流保护的 `POST /api/v1/auth/toolbox-session`，签发独立 token family。
3. 仅对该授权页面允许向 `http://127.0.0.1:*` 提交表单的 CSP。

4. 公开的 `GET /api/v1/auth/toolbox-capabilities` 协议检查，不包含账号或密钥数据。旧客户端可继续使用相同的授权路径。

5. `apply-caddy.cjs` 只对授权路径设置 `Referrer-Policy: origin`，避免 HTTPS 返回本机 HTTP 时发送 `Origin: null`。授权组件另设同样的 referrer meta，覆盖 OAuth 后的单页路由跳转；本机不放宽来源与 state 校验。安装脚本自动应用并验证 Caddy 配置，失败恢复备份。

`install-server.sh` 针对现有 `/opt/nftai/deploy/sub2api-theme/.src` 快照构建，只增加授权相关文件，保留现有计费代码；备份当前镜像、数据库、源码和部署配置，构建成功并验证币种一致后才切换服务。失败自动恢复原镜像。它也保留此前入口脚本 0755 权限修复，并为以后主题构建加上授权补丁步骤。

重新编译前后端并沿用站点现有备份、健康检查与回滚流程部署。服务器/Caddy 如另加 CSP，须对该路径同步放行本地回调表单。OAuth 登录方式沿用网站已启用的配置，未替你开启 GitHub/Google 登录。

如需撤回源码补丁，使用应用时打印的备份目录：

```sh
node portal-integration/rollback.cjs /absolute/path/to/sub2api /absolute/path/to/sub2api/.nftai-toolbox-backups/TIMESTAMP
```

回滚会先核对全部文件和备份校验值；发现补丁之后有其他修改时拒绝覆盖。该脚本只恢复源码，不回滚数据库或重启服务。

桌面会话具备用户原有权限，并非细粒度授权；授权页明确告知读取账号和密钥。未来可加专用 scopes。退出工具箱撤销该桌面 refresh token 并删除本机登录凭据，但已写入 AI 工具的 API 密钥仍继续有效；如需撤销调用权限，请在网站停用对应 API 密钥。

## 双平台构建

`.github/workflows/nftai-build.yml` 手动触发，分别构建 Windows x64 NSIS、macOS Apple Silicon DMG、macOS Intel DMG，生成 Actions artifacts，不会自动发布。

本机 Windows 可以构建 Windows 安装包。macOS 需要在 Mac 或 GitHub macOS runner 构建，不能在本机验证 macOS Keychain、安装与工具启动。当前工作流生成**未签名预览包**；公开发行前还需 Apple Developer 签名和公证、Windows 代码签名、自己的更新签名与发布地址。未配置你自己的更新源前，不自动更新。

## 第一版边界

- DSH 配置后在 DSH 内选择一次 NFTAI 模型；当前启动入口是 `dsh web`。
- Claude Code 使用 Anthropic Messages，Codex 使用 Responses，DSH/OpenCode 支持三种已适配协议。未知或复合分组不推测协议，显示暂不支持。
- 模型列表不包含可靠的价格和能力排序；默认推荐只在返回的模型中选取，不声称是最低价或最强模型。
- 缺少 Node.js 22+ 时自动下载官网最新 LTS 安装包，以官网 SHA-256 清单校验后打开系统安装程序；Windows 会显示系统安装确认，macOS 用户完成安装器后继续。拒绝校验失败的包，不运行远程 shell 脚本。拒绝安装、网络不可用或工具安装失败时显示错误，不假装完成。
- 版本探测不等于真实推理验证；未自动发起付费推理。
- `本次已配置` 仅表示当前会话成功应用，不伪装成跨重启的配置验证。
- 网站和客户端需要共同完成真实账号登录、刷新、退出、模型调用以及两平台实机验收，才可作为正式版本发布。

## 验收清单

0.1.1 本地已通过：前端类型检查、6 项工具箱测试、6 项原生测试、4 项网站补丁/回滚/后续升级保留测试、网站授权页 Vue 类型检查和生产界面构建。服务器构建还通过了授权协议 Go 测试 `TestToolboxCapabilities` 和现有人民币计费 Go 回归；未执行 macOS 安装验证。网站授权修复上线后，用户已确认可以登录进入工具箱首页；四款工具的实际安装、配置与推理需分别验收。

1. 浏览器未登录/已登录/已启用 2FA 的登录路径；错误 state 和错误 Origin 被拒。
2. 关闭浏览器、回调超时、凭据库锁定、登录过期、刷新失败时可理解的提示。
3. 同步所有分页、停用/到期密钥、未知分组、空模型列表；不同密钥的模型不串用。
4. 对四款工具逐一完成安装、再检测、应用、重启/新会话、发送用户批准的测试请求。
5. 写入前保留原配置；切换后与已有插件、MCP、提示词共存；失败可恢复。
6. Windows x64、macOS Apple Silicon、macOS Intel 安装与启动；退出后重开恢复登录。

0.1.2 验证：前端类型检查、8 项界面/配置测试、8 项原生回归测试通过；手动 Rust 实机检测通过，约 0.72 秒返回 DSH 桌面、Claude 桌面 5.11492.2.0、Codex 桌面 26.1106.6628.0。未触发真实配置写入、桌面启动或计费推理。
