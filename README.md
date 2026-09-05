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

## 技术栈

| 层级   | 技术方案                  |
|--------|---------------------------|
| 前端   | React 18 + TypeScript     |
| 后端   | Rust（Tauri 2）           |
| 构建   | Vite 6                    |
| 框架   | Tauri 2                   |

## 环境要求

构建前请确保已安装以下工具：

- [Node.js](https://nodejs.org/)（v18+）
- [Rust](https://www.rust-lang.org/tools/install)（stable 工具链）
- [Tauri 系统依赖](https://v2.tauri.app/start/prerequisites/)（按平台安装）

## 开发

```bash
# 安装前端依赖
npm install

# 以开发模式运行应用（支持热重载）
npm run tauri dev
```

## 构建

```bash
# 安装前端依赖
npm install

# 构建生产版本
npm run tauri build
```

构建产物位于 `src-tauri/target/release/bundle/` 目录。

## 项目结构

```
├── src/                  # React 前端
│   ├── components/       # UI 组件（UsagePanel、ProviderCard、Settings）
│   ├── hooks/            # 自定义 React Hooks
│   └── App.tsx
├── src-tauri/            # Rust 后端
│   ├── src/
│   │   ├── providers/    # LLM 服务商接口实现（DeepSeek、Kimi、智谱）
│   │   ├── commands.rs   # Tauri 命令处理器
│   │   ├── config.rs     # 配置管理
│   │   └── scheduler.rs  # 后台轮询调度器
│   └── Cargo.toml
```

## 许可证

本项目基于 MIT 许可证开源，详情请参阅 [LICENSE](LICENSE) 文件。
