# Repository Guidelines

## Project Structure & Module Organization
`src/` contains the React + TypeScript frontend. Keep page-level views in `src/pages/`, reusable UI in `src/components/`, 3D scene code in `src/components/3d/`, hooks in `src/hooks/`, and Tauri IPC wrappers/types in `src/lib/`. Shared styling lives in `src/styles/`.  
`src-tauri/` contains the Rust desktop backend: `src/ai/`, `assistant/`, `auth/`, `commands/`, `integrations/`, `scheduler/`, `system/`, and `voice/`, plus SQLite migrations in `src-tauri/migrations/`. Use `docs/` for design/spec notes and `scripts/` for helper scripts such as `scripts/download-whisper-model.sh`.

## Build, Test, and Development Commands
`npm run tauri dev` starts the desktop app locally with the Vite frontend and Tauri backend.  
`npm run build` runs `tsc` and creates the production frontend bundle in `dist/`.  
`npm run preview` serves the built frontend bundle for a quick browser check.  
`npm run tauri build` creates a desktop build.  
`cargo test --manifest-path src-tauri/Cargo.toml` runs Rust tests when backend logic changes.  
`cargo fmt --all --check --manifest-path src-tauri/Cargo.toml` is the expected Rust formatting check before review.

## Coding Style & Naming Conventions
Frontend code uses strict TypeScript (`tsconfig.json`) with ES modules, double quotes, and 2-space indentation. Name React components and pages in `PascalCase`, hooks as `useSomething`, and helpers/variables in `camelCase`. Keep Tauri command wrappers centralized in `src/lib/commands.ts`.  
Rust follows standard conventions: 4-space indentation, `snake_case` modules/functions, and small focused modules under `src-tauri/src/`.

## Testing Guidelines
There is no committed frontend test runner yet, so `npm run build` is the minimum validation for UI changes. For interactive work, verify the affected flow in `npm run tauri dev`.  
Add Rust unit tests close to the implementation or under `src-tauri/tests/` for non-trivial backend behavior. Bug fixes should include a regression test when practical.

## Commit & Pull Request Guidelines
Follow the existing Conventional Commit pattern seen in history, for example `feat(voice): shrink VoiceIndicator` or `feat(chat): redesign input bar`. Use imperative, scoped summaries.  
PRs should explain the user-visible change, list any new env vars or migration impacts, and include screenshots or short recordings for UI changes. Document manual verification steps and link the related issue when one exists.

## Security & Configuration Tips
Secrets belong in `.env` or the in-app Settings flow, never in source control. Update `.env.example` when adding required configuration. Do not commit local build artifacts such as `dist/` outputs you did not intend to ship or anything under `src-tauri/target/`.
