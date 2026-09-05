# Local LLM Token Usage Monitor Desktop

A lightweight desktop application for monitoring token usage and balances across multiple local LLM providers. Built with Tauri, React, TypeScript, and Rust.

![License](https://img.shields.io/github/license/hajifish/Local-LLM-Token-Usage-Monitor-Desktop)

## Features

- **Multi-Provider Support** — Monitor usage across multiple LLM platforms:
  - DeepSeek
  - Kimi (Moonshot)
  - Zhipu (GLM)
- **Real-Time Usage Tracking** — View token consumption and remaining balance at a glance
- **System Tray Integration** — Runs quietly in the background with a native tray icon
- **Auto-Start** — Optionally launch on system startup
- **Periodic Polling** — Automatically refreshes usage data on a configurable schedule
- **Low Resource Footprint** — Powered by Tauri's lightweight Rust backend

## Tech Stack

| Layer     | Technology                  |
|-----------|-----------------------------|
| Frontend  | React 18 + TypeScript       |
| Backend   | Rust (Tauri 2)              |
| Build     | Vite 6                      |
| Framework | Tauri 2                     |

## Prerequisites

Before building, ensure you have:

- [Node.js](https://nodejs.org/) (v18+)
- [Rust](https://www.rust-lang.org/tools/install) (stable)
- [Tauri system dependencies](https://v2.tauri.app/start/prerequisites/)

## Development

```bash
# Install frontend dependencies
npm install

# Run the app in development mode
npm run tauri dev
```

## Build

```bash
# Install frontend dependencies
npm install

# Build the production app
npm run tauri build
```

Built artifacts will be located in `src-tauri/target/release/bundle/`.

## Project Structure

```
├── src/                  # React frontend
│   ├── components/       # UI components (UsagePanel, ProviderCard, Settings)
│   ├── hooks/            # Custom React hooks
│   └── App.tsx
├── src-tauri/            # Rust backend
│   ├── src/
│   │   ├── providers/    # LLM provider implementations (DeepSeek, Kimi, Zhipu)
│   │   ├── commands.rs   # Tauri command handlers
│   │   ├── config.rs     # Configuration management
│   │   └── scheduler.rs  # Background polling scheduler
│   └── Cargo.toml
```

## License

This project is licensed under the MIT License — see the [LICENSE](LICENSE) file for details.
