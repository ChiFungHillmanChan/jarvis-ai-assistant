# Contributing to JARVIS

Thanks for your interest in contributing! This project is open to contributions of all kinds -- bug reports, feature suggestions, code improvements, and documentation fixes.

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/YOUR_USERNAME/jarvis-ai-assistant.git`
3. Install dependencies: `npm install`
4. Copy `.env.example` to `.env` and add your API keys
5. Run the app: `npm run tauri dev`

See the [README](README.md#getting-started) for full setup instructions and prerequisites.

## How to Contribute

### Reporting Bugs

- Open an [issue](https://github.com/ChiFungHillmanChan/jarvis-ai-assistant/issues) with a clear description
- Include steps to reproduce the bug
- Mention your macOS version and Node/Rust versions

### Suggesting Features

- Open an issue with the `enhancement` label
- Describe the feature and why it would be useful

### Submitting Code

1. Create a branch from `main`: `git checkout -b feat/your-feature`
2. Make your changes
3. Run `npm run build` to verify TypeScript compiles
4. Commit using [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `refactor:`, etc.
5. Push and open a Pull Request against `main`

## Code Style

- **TypeScript:** Strict mode, double quotes, 2-space indent
- **Rust:** Standard conventions, 4-space indent, `snake_case`
- **No emojis** in UI, code, or output
- Tauri command wrappers go in `src/lib/commands.ts`, types in `src/lib/types.ts`

## Project Structure

| Directory | What it contains |
|-----------|-----------------|
| `src/` | React frontend -- components, hooks, pages, styles |
| `src-tauri/src/` | Rust backend -- AI clients, integrations, commands, voice, scheduler |
| `src-tauri/migrations/` | SQLite schema migrations |

## Need Help?

Open an issue or start a discussion -- happy to help you get oriented.
