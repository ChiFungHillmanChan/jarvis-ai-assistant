# UI Shell Refresh - Design Spec

Date: 2026-07-11
Status: Approved (visual mockup signed off)
Scope: Desktop app (`jarvis-ai-assistant/`) frontend chrome only

## Goal

Make the app chrome (the frame around the center 3D scene) read as clean and
professional. Replace meaningless single-letter nav labels with real icons, fix
spacing/hierarchy/active-states, and restructure the Settings page so it is not a
wall of controls. The center holographic scene is already good and is out of
scope.

Success = the four surfaces below look like one deliberately-designed system, on
the existing holographic brand, with no typed-letter or ASCII-bracket glyphs left
in the chrome.

## Non-goals (explicit)

- Do NOT touch the center 3D scene (`components/3d/JarvisScene.tsx`) or any page's
  content logic (Dashboard, EmailPage, CalendarPage, etc.).
- Do NOT change the brand identity: dark `#060a14`, cyan glow `rgb(0,180,255)`,
  glassmorphism stays. This is a refinement, not a re-theme.
- Do NOT migrate to Tailwind / CSS modules / styled-components. Stay on the
  existing inline-style-object + CSS-variable pattern.
- Do NOT change Settings behavior, IPC wiring, or the controls themselves - only
  their layout/grouping and iconography.
- No new features. No speculative abstractions.

## Approach: thin shared token layer + lucide-react

1. **Add `lucide-react`** as the icon set (tree-shaken; only imported icons ship).
   React 18.3.1 compatible. GitHub and Notion brand marks are hand-drawn inline
   SVGs (lucide dropped brand logos) - kept in one small local module so the
   whole app imports icons from a single place.
2. **Extend `src/styles/global.css`** with a small set of design tokens (spacing,
   radii, named cyan/glass/border shades, one glow token). The theme already uses
   CSS variables, so this extends the existing pattern rather than introducing a
   new one. Every surface then reads from these tokens so spacing and cyan shades
   cannot drift apart across files.
3. **Redesign the four surfaces** against those tokens + icons.

### Dependency change

- `package.json`: add `lucide-react` (latest stable). Single new runtime dep.

### Icon access pattern

- One module `src/components/icons.tsx` that:
  - re-exports the lucide icons the app uses (named imports), and
  - exports two hand-drawn brand components: `GithubMark`, `NotionMark`.
- All chrome imports icons from this one module. Icons render at a consistent
  stroke width (1.75) and inherit `currentColor` so the cyan/muted states come
  from the parent, matching the existing WindowControls SVG pattern.

## Design tokens (add to `global.css`)

Add under `:root` alongside the existing `--font-mono` / `--font-sans`:

- Spacing: `--space-1: 4px` ... `--space-6: 24px` (4px scale).
- Radii: `--r-sm: 8px`, `--r-md: 12px`, `--r-lg: 16px`.
- Surfaces: `--surface: rgba(10,14,26,0.62)`, `--surface-solid: rgba(12,17,30,0.9)`.
- Borders: `--border: rgba(0,180,255,0.14)`, `--border-strong: rgba(0,180,255,0.30)`.
- Cyan: keep existing; add `--cyan-dim: rgba(0,180,255,0.62)`.
- Glow: `--glow: rgba(0,180,255,0.22)`.
- Text: `--muted: rgba(150,180,210,0.55)`.

Exact values may be tuned to match current theme constants during implementation;
the point is a single source of truth, not these literals.

## Surface 1 - Sidebar (`src/components/Sidebar.tsx`)

Replace the `navItems` `icon` letters with lucide/brand icon components. Mapping:

| id       | old letter | icon                  |
|----------|-----------|-----------------------|
| home     | `H`       | `LayoutDashboard`     |
| chat     | `>`       | `MessageSquare`       |
| email    | `M`       | `Mail`                |
| calendar | `C`       | `Calendar`            |
| github   | `G`       | `GithubMark` (drawn)  |
| notion   | `N`       | `NotionMark` (drawn)  |
| cron     | `T`       | `Clock`               |
| launch   | `B`       | `Rocket`              |
| settings | `S`       | `Settings` (gear)     |

- Rail width: 60px (from 56px) for comfortable 40px hit targets.
- **Active state**: left cyan bar (3px, glowing) + filled cyan tint + inset border,
  replacing the current tint-only state. Driven by existing `activeView` prop.
- Real hover tooltip showing the full label (mono, letter-spaced) - keep the
  existing `title` for accessibility and add a styled tooltip.
- `J` logo stays as the wordmark, restyled to a rounded gradient tile.
- Bottom chat toggle (`>_`) becomes a filled circular/rounded chat-icon button;
  keep `onChatToggle` wiring and the `Cmd+K` tooltip.
- Keep the `navItems` array shape and the `onNavigate`/`activeView` contract
  unchanged - only the rendered content and styles change.

## Surface 2 - Command box + Chat panel

### `src/components/CommandBar.tsx`
- Pill keeps its role (button that calls `onActivate`). Restyle: chat icon on the
  left, `Talk to JARVIS...` placeholder, a proper `Cmd K` keycap chip, and a round
  send button on the right. Uses `--r-lg`-based radius and the glow token.

### `src/components/ChatPanel.tsx`
- Replace ASCII pseudo-buttons in the header and input with icon buttons:
  - `NEW` -> `Plus`
  - `[>]` (expand) -> `Maximize2`
  - `[X]` (close) -> `X`
  - `[TH]` (think harder) -> `Sparkles` in a labeled pill
  - send arrow -> `ArrowUp` (replaces the existing inline send SVG for consistency)
- Header title `JARVIS CHAT` keeps mono/letter-spacing; add a small `Sparkles`
  mark. Keep all open/close/state wiring (`isOpen`, message rendering) unchanged.

## Surface 3 - Settings (`src/pages/Settings.tsx`)

Restructure layout only; preserve every existing control and its IPC wiring.

- Introduce a left **section nav** + right **content panel** (macOS System
  Settings style), replacing the current flat two-column scroll.
- Add local `activeSection` state; render only the active group.
- Proposed grouping of the existing ~14 sections (final grouping tunable in impl):
  - **General** - billing/upgrade state, language
  - **AI Providers** - LOCAL LLM ENDPOINTS, PROVIDER CHAIN, AI PROVIDER (LEGACY),
    AWS BEDROCK (keeps `LocalAiSetup` child intact)
  - **API Keys** - API KEYS
  - **Google** - GOOGLE SERVICES
  - **Notion** - NOTION
  - **GitHub** - GITHUB
  - **Obsidian** - OBSIDIAN
  - **Voice** - LANGUAGE & VOICE, VOICE, WAKE WORD
  - **Assistant** - ASSISTANT
  - **Appearance** - WALLPAPER MODE
- Each nav item gets a section icon (`Sliders`, `Cpu`, `Key`, `Globe`,
  `NotionMark`, `GithubMark`, `Gem`, `Mic`, `Bot`, `Palette`).
- Standardize a field row primitive (label + hint on the left, control on the
  right, hairline divider) so every control lines up. Controls (selects, toggles,
  text inputs, buttons) keep their current behavior and command calls.
- Keep the `LOADING SETTINGS...` state and lazy-load.

This is the largest change. It is a layout refactor: no settings value, command
wrapper, or side effect changes. If a section resists clean grouping, it may stay
as a labeled block inside its group rather than being split.

## Surface 4 - Top bar (`src/App.tsx` title bar + `WindowControls.tsx`)

- Align the `JARVIS` wordmark; keep `data-tauri-drag-region`.
- `UpdateButton` and the optional `SEND TO BACKGROUND` button become icon buttons
  (`RefreshCw`/`Download` and `Minimize2`) with tooltips; keep their handlers.
- Keep the custom macOS traffic-light `WindowControls` as-is (already SVG, on
  brand). Only spacing/alignment touched.

## Accessibility

- Every icon-only button gets an `aria-label` (and keeps `title` for tooltip).
- Active nav item gets `aria-current="page"`.
- Focus-visible outline on interactive chrome, cyan-tinted.

## Files touched

- `package.json` (add `lucide-react`)
- `src/styles/global.css` (add tokens)
- `src/components/icons.tsx` (new - icon barrel + brand marks)
- `src/components/Sidebar.tsx`
- `src/components/CommandBar.tsx`
- `src/components/ChatPanel.tsx`
- `src/pages/Settings.tsx`
- `src/App.tsx` (title-bar row only)
- `src/components/WindowControls.tsx` (spacing only, if needed)

No Rust / backend / command changes.

## Verification

- `npm run build` (Vite) passes; TypeScript clean.
- `npm run tauri dev` launches; visually confirm against the approved mockup:
  1. No typed-letter or ASCII-bracket glyph remains in sidebar, command box, chat
     header/input, or top bar.
  2. Active nav item shows the left cyan bar + glow; switching views moves it.
  3. Chat panel opens/closes and sends as before; new/expand/close/think-harder
     work.
  4. Settings section nav switches groups; every previously-present control is
     still reachable and still functions (spot-check API key save, provider
     select, a toggle).
  5. Bundle: confirm only imported lucide icons are included (tree-shaking), no
     full-library import.

## Open decisions deferred to implementation

- Exact token literals (match to current theme constants).
- Final Settings group boundaries for the two "legacy"/overlapping AI sections.
- Whether the chat FAB stays in the rail or is redundant given the command pill
  (leave as-is unless it looks duplicative in the running app).
