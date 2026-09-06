# Local LLM Token Usage Monitor Desktop

一款轻量级桌面应用，用于监控多个本地 LLM 服务商的 Token 用量与余额。基于 Tauri、React、TypeScript 和 Rust 构建。

![License](https://img.shields.io/github/license/hajifish/Local-LLM-Token-Usage-Monitor-Desktop)

## 功能预览

![功能预览](assets/screenshot-main.png)

## 功能特性

- **多服务商支持** — 同时监控多个 LLM 平台的使用情况：
  - DeepSeek
  - Kimi（月之暗面）
  - 智谱（GLM）
- **实时用量追踪** — 一目了然地查看 Token 消耗与剩余额度
- **系统集成托盘** — 在后台安静运行，原生支持系统托盘图标
- **开机自启** — 可选随系统启动自动运行
- **定时轮询** — 按可配置的时间间隔自动刷新用量数据
- **低资源占用** — 基于 Tauri 轻量级 Rust 后端驱动

## 下载安装

前往 [GitHub Releases](https://github.com/hajifish/Local-LLM-Token-Usage-Monitor-Desktop/releases) 下载最新版本。

| 平台 | 芯片 | 文件格式 |
|------|------|----------|
| macOS | Apple Silicon (M1/M2/M3/M4) | `.dmg` |
| Windows | x64 | `.msi` |

> 暂不支持 Intel 芯片的 Mac。如需 Intel 版本，请自行从源码构建。

### macOS 首次打开提示「已损坏」

由于应用未经 Apple 签名和公证，首次打开可能提示「应用已损坏」或「无法验证开发者」。解决方法：

1. 右键点击应用图标，选择「打开」
2. 在弹出的对话框中点击「打开」
3. 或者前往「系统设置 → 隐私与安全性」，点击「仍要打开」

## 配置文件与密钥安全

本应用会将各服务商的 API 密钥等配置保存在本地配置文件中。自本版本起，该配置文件由明文 JSON 升级为**机器绑定加密信封**，请在使用前了解以下几点。

### 配置文件路径

| 平台 | 路径 |
|------|------|
| macOS | `~/Library/Application Support/com.hajifish.llm-token-monitor/config.json` |
| Windows | `%APPDATA%/com.hajifish.llm-token-monitor/config.json` |
| Linux | `~/.local/share/com.hajifish.llm-token-monitor/config.json` |

### 文件不再是可直接编辑的明文

配置文件现在使用 XChaCha20-Poly1305 加密，密钥由「本机硬件标识 + 应用内置派生上下文」经 BLAKE3 派生得到，**解密密钥不会写入磁盘**。因此：

- **请勿再手工编辑该文件**——文件内容为密文，直接用文本编辑器修改不仅无效，还可能导致无法解密。
- 所有密钥的录入与修改请通过应用内的**设置页**完成（设置页仍以明文回显密钥，交互方式未变）。
- 首次启动本版本时，应用会自动将存量明文配置迁移为加密格式：迁移采用「临时文件 + 原子替换」并回读校验，迁移失败时会保留原文件不动、下次启动重试；迁移成功后文件权限收紧为仅当前用户可读写（Unix 下为 `0600`）。

### 换机、重装系统或系统标识变化

加密密钥与本机硬件标识绑定。一旦发生**更换电脑、重装系统、或系统标识发生变化**，原有密文将无法解密。此时：

- 应用**不会静默覆盖**原文件，而是将其自动备份为 `config.json.unreadable-<时间戳>.bak`，随后允许你重新录入密钥。
- 前端会显示提示横幅，告知配置文件无法读取、需要重新填写。

### 版本回退警告

> **重要：** 如果你回退到本次升级之前的旧版本，旧版本无法读取加密后的配置文件；在旧版本中执行保存操作会以明文覆盖该文件，导致已保存的加密密钥丢失。如确需回退到旧版本，请先在设置页记录或导出你的密钥。

### 威胁模型（它能防什么、不能防什么）

我们不夸大安全承诺。本方案的定位是**显著提高配置文件脱离本机后离线泄露的破解成本**，而非绝对安全：

- **能够防御**：配置文件副本脱离本机后被动泄露的场景，例如被云同步、系统备份、误当作附件发送等。
- **不能防御**：已经登录同一台机器、同一用户账户的攻击者。此类攻击者可以读取本机硬件标识，并从公开的源码中取得派生参数，从而在离线状态下重新计算出解密密钥；本方案也**不防御运行期的进程内存读取**。

简言之，本方案把明文密钥的暴露面从「任何拿到这个文件的人」收窄到「能在这台机器上以你的身份执行代码的人」。

### 日志位置（用于报障）

应用日志由 tauri-plugin-log 写入系统日志目录：

| 平台 | 路径 |
|------|------|
| macOS | `~/Library/Logs/com.hajifish.llm-token-monitor/` |
| Windows | `%APPDATA%/com.hajifish.llm-token-monitor/logs/` |
| Linux | `~/.local/share/com.hajifish.llm-token-monitor/logs/` |

> 日志中不包含任何密钥内容，可在报障时放心附上。

## 技术栈

| 层级   | 技术方案                  |
|--------|---------------------------|
| 前端   | React 18 + TypeScript     |
| 后端   | Rust（Tauri 2）           |
| 构建   | Vite 6                    |
| 框架   | Tauri 2                   |

## 开发

### 环境准备

- [Node.js](https://nodejs.org/) v18+
- [Rust](https://www.rust-lang.org/tools/install) stable 工具链
- [Tauri 系统依赖](https://v2.tauri.app/start/prerequisites/)

### 快速开始

```bash
# 1. 克隆仓库
git clone https://github.com/hajifish/Local-LLM-Token-Usage-Monitor-Desktop.git
cd Local-LLM-Token-Usage-Monitor-Desktop

# 2. 安装前端依赖
npm install

# 3. 以开发模式运行（支持热重载）
npm run tauri dev
```

### 构建生产版本

```bash
npm run tauri build
```

构建产物位于 `src-tauri/target/release/bundle/` 目录。

### 贡献指南

欢迎提交 Issue 和 Pull Request。详见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 项目结构

```
├── src/                        # React 前端
│   ├── components/
│   │   ├── ErrorBoundary.tsx   # 错误边界组件
│   │   ├── Settings.tsx        # 服务商配置页面
│   │   └── UsagePanel.tsx      # 用量展示面板
│   ├── hooks/
│   │   └── useUsage.ts         # 数据获取 Hook
│   ├── styles/
│   │   └── app.css             # 全局样式（浅色主题）
│   └── App.tsx                 # 主应用组件
├── src-tauri/                  # Rust 后端
│   ├── src/
│   │   ├── providers/          # LLM 服务商接口
│   │   │   ├── deepseek.rs     # DeepSeek
│   │   │   ├── kimi.rs         # Kimi（月之暗面）
│   │   │   ├── zhipu.rs        # 智谱（GLM）
│   │   │   ├── openai.rs       # OpenAI
│   │   │   └── anthropic.rs    # Anthropic
│   │   ├── commands.rs         # Tauri 命令处理器
│   │   ├── config.rs           # 配置管理
│   │   ├── models.rs           # 数据模型
│   │   ├── scheduler.rs        # 后台轮询调度器
│   │   └── tray.rs             # 系统托盘
│   └── Cargo.toml
└── .github/workflows/          # CI/CD
    ├── build.yml               # 构建验证
    └── release.yml             # 发布工作流
```

## 许可证

本项目基于 MIT 许可证开源，详情请参阅 [LICENSE](LICENSE) 文件。
