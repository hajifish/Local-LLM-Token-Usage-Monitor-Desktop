# Contributing to Local LLM Token Usage Monitor

Thank you for your interest in contributing! This document provides guidelines and information for contributors.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [How to Report Bugs](#how-to-report-bugs)
- [How to Suggest Features](#how-to-suggest-features)
- [How to Submit Pull Requests](#how-to-submit-pull-requests)
- [Development Setup](#development-setup)
- [Code Style Guidelines](#code-style-guidelines)

## Code of Conduct

By participating in this project, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

## How to Report Bugs

Bugs are tracked as [GitHub Issues](https://github.com/hajifish/Local-LLM-Token-Usage-Monitor-Desktop/issues).

Before creating an issue, please:

1. **Check existing issues** to see if the bug has already been reported
2. **Reproduce the bug** with the latest version of the app
3. **Gather information** that will help us understand and fix the issue

When submitting a bug report, include:

- A clear, descriptive title
- Steps to reproduce the behavior
- Expected behavior vs. actual behavior
- Screenshots or screen recordings (if applicable)
- Your system information (macOS version, architecture: Intel/Apple Silicon)
- App version (found in the app's About section or release notes)
- Relevant logs from the system console or app output

## How to Suggest Features

We welcome feature suggestions! To propose a new feature:

1. Open a [GitHub Issue](https://github.com/hajifish/Local-LLM-Token-Usage-Monitor-Desktop/issues/new) with the `enhancement` label
2. Describe the feature clearly and explain the problem it solves
3. Consider how it fits within the project's scope (lightweight LLM usage monitoring)
4. If applicable, provide mockups or examples of how it might work

Before suggesting a feature, please check if there's already an issue or discussion about it.

## How to Submit Pull Requests

### Before You Start

- For small fixes (typos, minor improvements), feel free to open a PR directly
- For larger changes, please open an issue first to discuss your approach with the maintainers
- Make sure your changes are well-tested before submitting

### Workflow

1. **Fork the repository** on GitHub
2. **Clone your fork** locally:
   ```bash
   git clone https://github.com/YOUR_USERNAME/Local-LLM-Token-Usage-Monitor-Desktop.git
   cd Local-LLM-Token-Usage-Monitor-Desktop
   ```
3. **Create a branch** for your changes:
   ```bash
   git checkout -b feature/your-feature-name
   # or
   git checkout -b fix/your-bug-fix
   ```
4. **Make your changes** and test them thoroughly
5. **Commit your changes** with clear, descriptive commit messages:
   ```
   feat: add support for new LLM provider
   fix: correct balance calculation for DeepSeek
   docs: update README with new provider
   ```
6. **Push to your fork** and open a Pull Request:
   ```bash
   git push origin feature/your-feature-name
   ```
7. **Describe your PR** clearly:
   - What problem does it solve?
   - How did you implement it?
   - Any relevant screenshots or context

### PR Review Process

- A maintainer will review your PR and may request changes
- Please respond to feedback promptly
- Once approved, a maintainer will merge your PR

## Development Setup

### Prerequisites

- [Node.js](https://nodejs.org/) (v18+)
- [Rust](https://www.rust-lang.org/tools/install) (stable toolchain)
- [Tauri system dependencies](https://v2.tauri.app/start/prerequisites/) (platform-specific)

### Getting Started

```bash
# Install frontend dependencies
npm install

# Run the app in development mode (with hot reload)
npm run tauri dev
```

### Project Structure

```
├── src/                    # React frontend (TypeScript)
│   ├── components/         # UI components
│   │   ├── UsagePanel.tsx  # Main usage display
│   │   ├── ProviderCard.tsx # Individual provider card
│   │   └── Settings.tsx    # Settings panel
│   ├── hooks/              # Custom React hooks
│   └── App.tsx             # Root component
├── src-tauri/              # Rust backend
│   ├── src/
│   │   ├── providers/      # LLM provider API implementations
│   │   ├── commands.rs     # Tauri command handlers
│   │   ├── config.rs       # Configuration management
│   │   ├── scheduler.rs    # Background polling scheduler
│   │   └── tray.rs         # System tray integration
│   └── Cargo.toml          # Rust dependencies
```

### Key Technologies

| Layer     | Technology            |
|-----------|----------------------|
| Frontend  | React 18 + TypeScript |
| Backend   | Rust (Tauri 2)       |
| Build     | Vite 6               |
| Framework | Tauri 2              |

### Common Commands

```bash
# Frontend development (without Tauri)
npm run dev

# Build for production
npm run tauri build

# Type check
npx tsc --noEmit
```

## Code Style Guidelines

### General

- Use consistent indentation (2 spaces for TypeScript/JSX, 4 spaces for Rust)
- Keep lines under 120 characters where reasonable
- Write self-documenting code; add comments only when explaining "why" not "what"
- Remove dead code and unused imports

### TypeScript / React

- Use TypeScript strict mode
- Prefer functional components with hooks
- Use descriptive variable and function names in camelCase
- Component names should be PascalCase
- Type all props and return values explicitly
- Avoid `any` types — use proper TypeScript types

### Rust

- Follow standard Rust formatting (`cargo fmt`)
- Run `cargo clippy` and address warnings before submitting
- Use `Result` and `Option` for error handling; avoid `unwrap()` in production code
- Keep functions focused and reasonably sized
- Add doc comments (`///`) for public functions and structs

### Commit Messages

Use [Conventional Commits](https://www.conventionalcommits.org/) format:

```
<type>: <description>

[optional body]
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`

Examples:
- `feat: add polling interval configuration`
- `fix: handle network timeout gracefully`
- `docs: add setup instructions for Linux`

## Questions?

If you have questions about contributing, feel free to open a [GitHub Discussion](https://github.com/hajifish/Local-LLM-Token-Usage-Monitor-Desktop/discussions) or ask in an issue.

Thank you for helping improve Local LLM Token Usage Monitor!
