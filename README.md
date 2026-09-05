# Local LLM Token Usage Monitor Desktop

一款轻量级桌面应用，用于监控多个本地 LLM 服务商的 Token 用量与余额。基于 Tauri、React、TypeScript 和 Rust 构建。

![License](https://img.shields.io/github/license/hajifish/Local-LLM-Token-Usage-Monitor-Desktop)

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

> 暂不支持 Intel 芯片的 Mac。如需 Intel 版本，请自行从源码构建。

### macOS 首次打开提示「已损坏」

由于应用未经 Apple 签名和公证，首次打开可能提示「应用已损坏」或「无法验证开发者」。解决方法：

1. 右键点击应用图标，选择「打开」
2. 在弹出的对话框中点击「打开」
3. 或者前往「系统设置 → 隐私与安全性」，点击「仍要打开」

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
