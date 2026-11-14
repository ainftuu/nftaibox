# NFTAI 工具箱

面向新手的 AI 编程工具箱，首批支持 DeepSeek Harness、Claude Code、Codex、OpenCode。

浏览器登录 NFTAI → 同步 API 密钥 → 选择工具和模型 → 安装并应用配置。

支持 Windows 与 macOS。具体构建、网站授权接入、验证情况及第一版边界请阅读 [NFTAI-README.md](NFTAI-README.md)。

本项目基于 CC Switch 修改，保留 MIT 许可。上游来源和提交见 [UPSTREAM.json](UPSTREAM.json)，原说明见 [docs/upstream-README.md](docs/upstream-README.md)。

当前为 0.1.5 预览版，修复 Chromium 表单策略拦截回首页跳转；授权登录成功后浏览器立即跳转官网首页；概览余额使用钱币图标，余额/密钥图标统一黑底荧光绿，并移除设置页上游项目链接。保留 0.1.2 的桌面版识别、重新检测及 0.1.1 授权修复。macOS 包需在 Mac 或 GitHub macOS runner 构建。
