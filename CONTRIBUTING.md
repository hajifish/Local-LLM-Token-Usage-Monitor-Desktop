# 为 Local LLM Token Usage Monitor 贡献代码

感谢您对本项目的关注！本文档为贡献者提供指南和相关信息。

## 目录

- [行为准则](#行为准则)
- [如何报告 Bug](#如何报告-bug)
- [如何建议新功能](#如何建议新功能)
- [如何提交 Pull Request](#如何提交-pull-request)
- [开发环境搭建](#开发环境搭建)
- [代码风格指南](#代码风格指南)

## 行为准则

参与本项目即表示您同意遵守我们的[行为准则](CODE_OF_CONDUCT.md)。

## 如何报告 Bug

Bug 通过 [GitHub Issues](https://github.com/hajifish/Local-LLM-Token-Usage-Monitor-Desktop/issues) 进行跟踪。

创建 Issue 前，请：

1. **查看现有 Issue**，确认该 Bug 是否已被报告
2. **使用最新版本**的应用复现该 Bug
3. **收集信息**，帮助我们理解和修复问题

提交 Bug 报告时，请提供：

- 清晰、描述性的标题
- 复现步骤
- 预期行为与实际行为
- 截图或屏幕录制（如有）
- 您的系统信息（macOS 版本，架构：Intel / Apple Silicon）
- 应用版本（可在"关于"或发布说明中找到）
- 系统控制台或应用输出中的相关日志

## 如何建议新功能

我们欢迎功能建议！提议新功能请：

1. 在 [GitHub Issue](https://github.com/hajifish/Local-LLM-Token-Usage-Monitor-Desktop/issues/new) 中使用 `enhancement` 标签
2. 清晰描述该功能，并说明它解决的问题
3. 考虑它如何融入项目范围（轻量级 LLM 用量监控）
4. 如适用，提供模拟图或示例

建议新功能前，请先查看是否已有相关 Issue 或讨论。

## 如何提交 Pull Request

### 开始之前

- 对于小改动（错别字、小改进），可以直接提交 PR
- 对于较大改动，请先开 Issue 与维护者讨论方案
- 提交前请确保您的改动经过充分测试

### 工作流程

1. **Fork 仓库**：在 GitHub 上 Fork 本项目
2. **克隆到本地**：
   ```bash
   git clone https://github.com/YOUR_USERNAME/Local-LLM-Token-Usage-Monitor-Desktop.git
   cd Local-LLM-Token-Usage-Monitor-Desktop
   ```
3. **创建分支**：
   ```bash
   git checkout -b feature/your-feature-name
   # 或
   git checkout -b fix/your-bug-fix
   ```
4. **进行修改**并充分测试
5. **提交更改**，使用清晰、描述性的提交信息：
   ```
   feat: add support for new LLM provider
   fix: correct balance calculation for DeepSeek
   docs: update README with new provider
   ```
6. **推送到 Fork** 并发起 Pull Request：
   ```bash
   git push origin feature/your-feature-name
   ```
7. **清晰描述您的 PR**：
   - 解决了什么问题？
   - 如何实现的？
   - 相关截图或上下文

### PR 审核流程

- 维护者会审核您的 PR，并可能要求修改
- 请及时响应反馈
- 审核通过后，维护者将合并您的 PR

## 开发环境搭建

### 环境要求

- [Node.js](https://nodejs.org/)（v18+）
- [Rust](https://www.rust-lang.org/tools/install)（stable 工具链）
- [Tauri 系统依赖](https://v2.tauri.app/start/prerequisites/)（按平台安装）

### 开始开发

```bash
# 安装前端依赖
npm install

# 以开发模式运行应用（支持热重载）
npm run tauri dev
```

### 项目结构

```
├── src/                    # React 前端（TypeScript）
│   ├── components/         # UI 组件
│   │   ├── UsagePanel.tsx  # 主用量展示面板
│   │   ├── ProviderCard.tsx # 单个服务商卡片
│   │   └── Settings.tsx    # 设置面板
│   ├── hooks/              # 自定义 React Hooks
│   └── App.tsx             # 根组件
├── src-tauri/              # Rust 后端
│   ├── src/
│   │   ├── providers/      # LLM 服务商 API 实现
│   │   ├── commands.rs     # Tauri 命令处理器
│   │   ├── config.rs       # 配置管理
│   │   ├── scheduler.rs    # 后台轮询调度器
│   │   └── tray.rs         # 系统托盘集成
│   └── Cargo.toml          # Rust 依赖配置
```

### 核心技术

| 层级   | 技术方案              |
|--------|-----------------------|
| 前端   | React 18 + TypeScript |
| 后端   | Rust（Tauri 2）       |
| 构建   | Vite 6                |
| 框架   | Tauri 2               |

### 常用命令

```bash
# 前端开发（不启动 Tauri）
npm run dev

# 构建生产版本
npm run tauri build

# 类型检查
npx tsc --noEmit
```

## 代码风格指南

### 通用规范

- 使用一致的缩进（TypeScript/JSX 使用 2 空格，Rust 使用 4 空格）
- 合理控制每行不超过 120 个字符
- 编写自解释的代码；注释只用于解释"为什么"，而非"是什么"
- 删除无用代码和未使用的导入

### TypeScript / React

- 使用 TypeScript 严格模式
- 优先使用函数式组件和 Hooks
- 使用描述性的变量名和函数名，采用 camelCase 命名
- 组件名使用 PascalCase
- 明确标注所有 props 和返回值类型
- 避免使用 `any` 类型，使用准确的 TypeScript 类型

### Rust

- 遵循标准 Rust 格式化（`cargo fmt`）
- 提交前运行 `cargo clippy` 并处理所有警告
- 使用 `Result` 和 `Option` 进行错误处理；避免在生产代码中使用 `unwrap()`
- 保持函数专注且大小合理
- 为公开函数和结构体添加文档注释（`///`）

### 提交信息

使用 [Conventional Commits](https://www.conventionalcommits.org/) 格式：

```
<type>: <description>

[optional body]
```

类型：`feat`、`fix`、`docs`、`style`、`refactor`、`test`、`chore`

示例：
- `feat: add polling interval configuration`
- `fix: handle network timeout gracefully`
- `docs: add setup instructions for Linux`

## 有问题？

如对贡献有任何疑问，欢迎发起 [GitHub Discussion](https://github.com/hajifish/Local-LLM-Token-Usage-Monitor-Desktop/discussions) 或在 Issue 中提问。

感谢您为 Local LLM Token Usage Monitor 做出的贡献！
