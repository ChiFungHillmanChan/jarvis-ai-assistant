# UI Shell Refresh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the desktop app's typed-letter/ASCII chrome (sidebar, command box, chat panel, top bar, settings) with real icons and a cleaner layout, on the existing holographic theme.

**Architecture:** Add `lucide-react` for icons behind a single `icons.tsx` barrel (with two hand-drawn brand marks), add a thin design-token layer to `global.css`, then restyle each chrome surface against those tokens. No backend, no page-content, no theme-identity changes.

**Tech Stack:** React 18.3 + TypeScript + Vite, inline style objects + CSS variables, `lucide-react`.

## Global Constraints

- No emojis anywhere (UI, code, commits). (root + repo CLAUDE.md)
- Keep the holographic brand: dark `#0a0e1a`/`#060a14`, cyan `rgb(0,180,255)`, glassmorphism. Refinement only, not a re-theme.
- Stay on inline-style-object + CSS-variable pattern. No Tailwind / CSS modules / styled-components.
- Do NOT touch the center 3D scene (`components/3d/*`) or any page content logic. Settings is layout/iconography only - every existing control and its IPC wiring is preserved.
- No new runtime deps except `lucide-react`.
- Verification gate for every task: `npm run build` (runs `tsc && vite build`) must pass. There is no frontend unit-test runner in this repo; do not add one (YAGNI). Behavioral checks are done visually in `npm run tauri dev` against the approved mockup.
- Icons render at stroke-width 1.75, `fill="none"`, inheriting `currentColor`.

---

### Task 1: Add lucide-react and the icon barrel

**Files:**
- Modify: `package.json` (add dependency)
- Create: `src/components/icons.tsx`

**Interfaces:**
- Produces: named icon components re-exported from `lucide-react`
  (`LayoutDashboard, MessageSquare, Mail, Calendar, Clock, Rocket, Settings,
  Plus, X, Maximize2, Sparkles, ArrowUp, Minimize2, Sliders, Cpu, Key, Globe,
  Gem, Mic, Bot, Palette`) plus two local components `GithubMark` and
  `NotionMark`, each `(props: { size?: number }) => JSX.Element`.

- [ ] **Step 1: Install the dependency**

Run: `cd /Users/hillmanchan/Desktop/jarvis-projects/jarvis-ai-assistant && npm install lucide-react`
Expected: `lucide-react` added to `package.json` dependencies, no peer-dep errors (React 18 satisfied).

- [ ] **Step 2: Create the icon barrel**

Create `src/components/icons.tsx`:

```tsx
// Central icon module. Everything in the app chrome imports icons from here so
// stroke weight and brand marks stay consistent. lucide dropped brand logos, so
// GitHub and Notion are hand-drawn to match lucide's 24x24 / stroke-1.75 grid.
export {
  LayoutDashboard,
  MessageSquare,
  Mail,
  Calendar,
  Clock,
  Rocket,
  Settings,
  Plus,
  X,
  Maximize2,
  Sparkles,
  ArrowUp,
  Minimize2,
  Sliders,
  Cpu,
  KeyRound as Key,
  Globe,
  Gem,
  Mic,
  Bot,
  Palette,
} from "lucide-react";

interface MarkProps { size?: number }

export function GithubMark({ size = 20 }: MarkProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none"
      stroke="currentColor" strokeWidth={1.75} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M9 19c-5 1.5-5-2.5-7-3m14 6v-3.87a3.37 3.37 0 0 0-.94-2.61c3.14-.35 6.44-1.54 6.44-7A5.44 5.44 0 0 0 20 4.77 5.07 5.07 0 0 0 19.91 1S18.73.65 16 2.48a13.38 13.38 0 0 0-7 0C6.27.65 5.09 1 5.09 1A5.07 5.07 0 0 0 5 4.77a5.44 5.44 0 0 0-1.5 3.78c0 5.42 3.3 6.61 6.44 7A3.37 3.37 0 0 0 9 18.13V22" />
    </svg>
  );
}

export function NotionMark({ size = 20 }: MarkProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none"
      stroke="currentColor" strokeWidth={1.75} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="3" y="3" width="18" height="18" rx="2.5" />
      <path d="M8.5 8.2v7.6" />
      <path d="M8.5 8.2l7 7.6" />
      <path d="M15.5 8.2v7.6" />
    </svg>
  );
}
```

- [ ] **Step 3: Verify build passes**

Run: `npm run build`
Expected: PASS (tsc clean, vite build succeeds). Confirms lucide types resolve and the barrel compiles.

---

### Task 2: Add design tokens to global.css

**Files:**
- Modify: `src/styles/global.css:34-38` (the `Border radii` block area, extend `:root`)

**Interfaces:**
- Produces: CSS variables consumed by later tasks:
  `--space-1..--space-6`, `--r-sm/--r-md/--r-lg` (already present; keep),
  `--surface`, `--surface-solid`, `--border-strong`, `--cyan-dim`, `--glow`,
  `--muted`.

- [ ] **Step 1: Add tokens under :root**

In `src/styles/global.css`, inside `:root` (after the existing `--radius-lg: 12px;` line, before the closing `}` at line 38), add:

```css
  /* Spacing scale (4px base) */
  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-5: 20px;
  --space-6: 24px;

  /* Named radii (aliases for the refreshed chrome) */
  --r-sm: 8px;
  --r-md: 12px;
  --r-lg: 16px;

  /* Surfaces + borders + glow (refreshed chrome) */
  --surface: rgba(10, 14, 26, 0.62);
  --surface-solid: rgba(12, 17, 30, 0.94);
  --border-strong: rgba(0, 180, 255, 0.30);
  --cyan-dim: rgba(0, 180, 255, 0.62);
  --muted: rgba(150, 180, 210, 0.55);
  --glow: rgba(0, 180, 255, 0.22);
```

- [ ] **Step 2: Verify build passes**

Run: `npm run build`
Expected: PASS. (CSS variables are inert until used; this just confirms no syntax error.)

---

### Task 3: Sidebar redesign

**Files:**
- Modify: `src/components/Sidebar.tsx` (whole file)

**Interfaces:**
- Consumes: icon components from `./icons` (Task 1); tokens from `global.css` (Task 2).
- Props contract UNCHANGED: `{ activeView: string; onNavigate: (view: string) => void; onChatToggle: () => void }`. `navItems` ids stay `home/chat/email/calendar/github/notion/cron/launch/settings`.

- [ ] **Step 1: Rewrite Sidebar.tsx**

Replace the entire file with:

```tsx
import { memo } from "react";
import {
  LayoutDashboard, MessageSquare, Mail, Calendar, Clock, Rocket, Settings,
  GithubMark, NotionMark,
} from "./icons";

interface SidebarProps {
  activeView: string;
  onNavigate: (view: string) => void;
  onChatToggle: () => void;
}

const ICON = 19;
const navItems = [
  { id: "home", label: "HOME", Icon: (p: { size: number }) => <LayoutDashboard size={p.size} /> },
  { id: "chat", label: "CHAT", Icon: (p: { size: number }) => <MessageSquare size={p.size} /> },
  { id: "email", label: "MAIL", Icon: (p: { size: number }) => <Mail size={p.size} /> },
  { id: "calendar", label: "CALENDAR", Icon: (p: { size: number }) => <Calendar size={p.size} /> },
  { id: "github", label: "GITHUB", Icon: (p: { size: number }) => <GithubMark size={p.size} /> },
  { id: "notion", label: "NOTION", Icon: (p: { size: number }) => <NotionMark size={p.size} /> },
  { id: "cron", label: "CRON", Icon: (p: { size: number }) => <Clock size={p.size} /> },
  { id: "launch", label: "BETA", Icon: (p: { size: number }) => <Rocket size={p.size} /> },
];

export default memo(function Sidebar({ activeView, onNavigate, onChatToggle }: SidebarProps) {
  const renderItem = (item: { id: string; label: string; Icon: (p: { size: number }) => JSX.Element }) => {
    const active = activeView === item.id;
    return (
      <button
        key={item.id}
        onClick={() => onNavigate(item.id)}
        style={{ ...styles.navButton, ...(active ? styles.navButtonActive : {}) }}
        title={item.label}
        aria-label={item.label}
        aria-current={active ? "page" : undefined}
      >
        {active && <span style={styles.activeBar} aria-hidden="true" />}
        <item.Icon size={ICON} />
      </button>
    );
  };

  const settingsActive = activeView === "settings";
  return (
    <div style={styles.container}>
      <div style={styles.logo}><span style={styles.logoText}>J</span></div>
      <nav style={styles.nav}>{navItems.map(renderItem)}</nav>
      <button
        onClick={() => onNavigate("settings")}
        style={{ ...styles.navButton, ...(settingsActive ? styles.navButtonActive : {}), marginTop: "auto" }}
        title="SETTINGS"
        aria-label="SETTINGS"
        aria-current={settingsActive ? "page" : undefined}
      >
        {settingsActive && <span style={styles.activeBar} aria-hidden="true" />}
        <Settings size={ICON} />
      </button>
      <button onClick={onChatToggle} style={styles.chatButton} title="Chat (Cmd+K)" aria-label="Open chat">
        <MessageSquare size={18} />
      </button>
    </div>
  );
});

const styles: Record<string, React.CSSProperties> = {
  container: { width: 60, height: "100%", display: "flex", flexDirection: "column", alignItems: "center", padding: "12px 0", borderRight: "1px solid var(--border-primary)", background: "var(--surface)", backdropFilter: "blur(12px)", gap: 6 },
  logo: { width: 34, height: 34, borderRadius: 10, display: "flex", alignItems: "center", justifyContent: "center", marginBottom: 14, background: "linear-gradient(150deg, rgba(0,180,255,0.95), rgba(10,134,196,0.9))", boxShadow: "0 0 18px rgba(0,180,255,0.4)" },
  logoText: { color: "#04121c", fontFamily: "var(--font-mono)", fontSize: 16, fontWeight: 700 },
  nav: { display: "flex", flexDirection: "column", gap: 6, alignItems: "center" },
  navButton: { position: "relative", width: 42, height: 42, border: "1px solid transparent", borderRadius: 11, background: "transparent", cursor: "pointer", display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-muted)", transition: "all 0.15s ease" },
  navButtonActive: { background: "rgba(0,180,255,0.12)", borderColor: "var(--border-strong)", color: "var(--text-primary)", boxShadow: "0 0 16px -4px rgba(0,180,255,0.6), inset 0 0 0 1px var(--border-primary)" },
  activeBar: { position: "absolute", left: -13, top: "50%", transform: "translateY(-50%)", width: 3, height: 20, borderRadius: 2, background: "rgb(0,180,255)", boxShadow: "0 0 10px rgb(0,180,255)" },
  chatButton: { width: 42, height: 42, borderRadius: 12, border: "none", cursor: "pointer", display: "flex", alignItems: "center", justifyContent: "center", marginTop: 8, color: "#04121c", background: "linear-gradient(150deg, rgba(0,180,255,0.95), rgba(10,134,196,0.9))", boxShadow: "0 0 16px rgba(0,180,255,0.45)" },
};
```

Note: hover color transitions handled by inline `transition`; the icon color follows `currentColor` from `navButton`/`navButtonActive`. Nav no longer uses `flex:1` on the `<nav>`; `marginTop:"auto"` on the Settings button pushes it (and the chat FAB) to the bottom.

- [ ] **Step 2: Verify build passes**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 3: Visual check**

Run `npm run tauri dev`. Confirm: rail shows 8 icons + gear + chat FAB, no letters; the active view shows the left cyan bar + glow; clicking each nav item still switches views; hovering shows the tooltip label.

---

### Task 4: Command box + Chat panel

**Files:**
- Modify: `src/components/CommandBar.tsx` (whole file)
- Modify: `src/components/ChatPanel.tsx:45-119` (JSX for header actions + input controls) and `:121-142` (styles for those buttons)

**Interfaces:**
- Consumes: icons from `./icons` (Task 1).
- Props/behavior contracts UNCHANGED (`CommandBar.onActivate`; `ChatPanel` props, `clearChat`, `onNavigateToChat`, `onClose`, `handleSubmit`, `thinkHarder` toggle).

- [ ] **Step 1: Rewrite CommandBar.tsx**

```tsx
import { MessageSquare, ArrowUp } from "./icons";

interface CommandBarProps { onActivate: () => void; }
export default function CommandBar({ onActivate }: CommandBarProps) {
  return (
    <div style={styles.container}>
      <button onClick={onActivate} style={styles.bar} aria-label="Talk to JARVIS">
        <span style={styles.icon}><MessageSquare size={18} /></span>
        <span style={styles.text}>Talk to JARVIS...</span>
        <span style={styles.kbd}>&#8984; K</span>
        <span style={styles.send}><ArrowUp size={16} /></span>
      </button>
    </div>
  );
}
const styles: Record<string, React.CSSProperties> = {
  container: { padding: "8px 16px 16px 16px" },
  bar: { width: "100%", display: "flex", alignItems: "center", gap: 12, padding: "0 8px 0 16px", height: 46, background: "rgba(8,14,26,0.7)", border: "1px solid var(--border-strong)", borderRadius: 23, cursor: "pointer", boxShadow: "0 0 30px -12px rgba(0,180,255,0.5)", transition: "border-color 0.2s ease" },
  icon: { color: "rgb(0,180,255)", display: "flex" },
  text: { color: "var(--muted)", fontSize: 13.5, flex: 1, textAlign: "left" as const },
  kbd: { color: "var(--cyan-dim)", fontFamily: "var(--font-mono)", fontSize: 10, letterSpacing: 1, border: "1px solid var(--border-strong)", borderRadius: 6, padding: "4px 8px", background: "rgba(0,180,255,0.06)" },
  send: { width: 32, height: 32, borderRadius: "50%", display: "flex", alignItems: "center", justifyContent: "center", color: "#04121c", background: "linear-gradient(150deg, rgb(0,180,255), #0a86c4)", flexShrink: 0 },
};
```

- [ ] **Step 2: Update ChatPanel header + input icons**

In `src/components/ChatPanel.tsx`, add to the imports at the top (after line 3):

```tsx
import { Plus, Maximize2, X, Sparkles, ArrowUp } from "./icons";
```

Replace the header actions block (lines 47-54) with:

```tsx
      <div style={styles.header}>
        <span className="system-text" style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Sparkles size={14} /> JARVIS
        </span>
        <div style={styles.headerActions}>
          <button onClick={clearChat} style={styles.iconBtn} title="New chat" aria-label="New chat"><Plus size={16} /></button>
          <button onClick={onNavigateToChat} style={styles.iconBtn} title="Open full view" aria-label="Open full view"><Maximize2 size={15} /></button>
          <button onClick={onClose} style={styles.iconBtn} title="Close" aria-label="Close"><X size={16} /></button>
        </div>
      </div>
```

Replace the think-harder button (lines 102-109) with:

```tsx
          <button
            type="button"
            onClick={() => setThinkHarder((v) => !v)}
            style={thinkHarder ? styles.thinkHarderButtonActive : styles.thinkHarderButton}
            title="Think harder (routes to Sonnet 4.6)"
            aria-pressed={thinkHarder}
          >
            <Sparkles size={12} /> Think harder
          </button>
```

Replace the submit button's inline `<svg>` (lines 110-114) with:

```tsx
          <button type="submit" style={styles.sendButton} disabled={!input.trim()} aria-label="Send">
            <ArrowUp size={16} />
          </button>
```

- [ ] **Step 3: Update ChatPanel button styles**

In the `styles` object (lines 121-142), add an `iconBtn` entry and adjust the think-harder buttons to lay out icon + text. Replace `headerBtn` usage is gone; add:

```tsx
  iconBtn: { display: "flex", alignItems: "center", justifyContent: "center", width: 28, height: 28, borderRadius: 7, background: "transparent", border: "1px solid transparent", color: "var(--muted)", cursor: "pointer" },
```

Update `sendButton` color to cyan icon (already cyan container); set `color: "rgb(0,180,255)"`. Update `thinkHarderButton` / `thinkHarderButtonActive` to include `display:"flex", alignItems:"center", gap:6` and drop the fixed mono glyph sizing so the label + icon sit inline. Keep their colors/borders as-is.

Note: the old `headerBtn` style key can stay unused or be removed; removing it is cleanup of code THIS task made unused, which is allowed.

- [ ] **Step 4: Verify build passes**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 5: Visual check**

Run `npm run tauri dev`. Open chat (Cmd+K). Confirm: command pill shows chat icon + keycap + send; chat header shows plus/expand/close icons; think-harder is a sparkles pill that toggles; send is an up-arrow and still sends; no `[X] [>] [TH] NEW >_` text remains.

---

### Task 5: Top bar alignment + Send-to-background icon

**Files:**
- Modify: `src/App.tsx:344-358` (title bar JSX) and `:393-398` (title-bar styles)

**Interfaces:**
- Consumes: `Minimize2` from `./components/icons`.
- Behavior UNCHANGED: `UpdateButton`, `handleLowerToBackground`, `WindowControls` keep their handlers. `UpdateButton` and `WindowControls` are NOT restyled (UpdateButton shows download progress; traffic lights are already on brand).

- [ ] **Step 1: Import the icon**

In `src/App.tsx`, add to the component imports:

```tsx
import { Minimize2 } from "./components/icons";
```

- [ ] **Step 2: Icon-ify the send-to-background button + align title**

Replace the title-bar block (lines 344-358) with:

```tsx
        <div data-tauri-drag-region style={styles.titleBar}>
          <span data-tauri-drag-region style={styles.titleText}>JARVIS{wallpaperActive ? " / WALLPAPER" : ""}</span>
          <div data-tauri-drag-region="false" style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <UpdateButton />
            {wallpaperActive && wallpaperRaised && (
              <button onClick={handleLowerToBackground} style={styles.lowerBtn} title="Send to background" aria-label="Send to background">
                <Minimize2 size={13} /> Background
              </button>
            )}
            <WindowControls />
          </div>
        </div>
```

- [ ] **Step 3: Update lowerBtn style for icon + label**

In `styles` (line 398), replace `lowerBtn` with:

```tsx
  lowerBtn: { display: "flex", alignItems: "center", gap: 6, background: "rgba(0,180,255,0.1)", border: "1px solid var(--border-strong)", borderRadius: 7, padding: "4px 10px", color: "var(--cyan-dim)", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 10, letterSpacing: 1 },
```

- [ ] **Step 4: Verify build passes**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 5: Visual check**

Run `npm run tauri dev`. Confirm the top bar title is aligned and the window controls sit cleanly at the right. (Send-to-background only shows in wallpaper mode; skip unless testing wallpaper.)

---

### Task 6: Settings section navigation

**Files:**
- Modify: `src/pages/Settings.tsx` (render return `:421-836` and `styles` `:839-859`)

**Interfaces:**
- Consumes: icons from `../components/icons` (Task 1).
- Behavior UNCHANGED: every existing state var, handler, and IPC call is kept verbatim. Only the JSX wrapping (two flat columns -> section nav + active-group panel) and styles change. The `if (!loaded)` LOADING branch (lines 400-406) stays.

- [ ] **Step 1: Add section state + icon imports**

At the top of `src/pages/Settings.tsx`, add to imports:

```tsx
import { Cpu, Key, Globe, Gem, Mic, Bot, Palette, GithubMark, NotionMark } from "../components/icons";
```

Inside the component, after the existing `useState` declarations (near line 101), add:

```tsx
  const [section, setSection] = useState<string>("ai");
```

Define the nav model just before the `return` of the loaded view (after line 419):

```tsx
  const SECTIONS = [
    { id: "ai", label: "AI Providers", Icon: () => <Cpu size={17} /> },
    { id: "keys", label: "API Keys", Icon: () => <Key size={17} /> },
    { id: "google", label: "Google", Icon: () => <Globe size={17} /> },
    { id: "notion", label: "Notion", Icon: () => <NotionMark size={17} /> },
    { id: "github", label: "GitHub", Icon: () => <GithubMark size={17} /> },
    { id: "obsidian", label: "Obsidian", Icon: () => <Gem size={17} /> },
    { id: "voice", label: "Voice", Icon: () => <Mic size={17} /> },
    { id: "assistant", label: "Assistant", Icon: () => <Bot size={17} /> },
    { id: "appearance", label: "Appearance", Icon: () => <Palette size={17} /> },
    { id: "account", label: "Account", Icon: () => <Palette size={17} /> },
  ];
```

(Account uses a distinct icon in impl - swap `Palette` for a suitable lucide icon like `CreditCard` if desired; add it to the barrel first.)

- [ ] **Step 2: Replace the layout shell**

Replace the outer render (lines 421-835, the `<div style={styles.container}>...</div>` that holds the `SETTINGS` title + the two-column grid) with a nav + content shell. Keep the EXISTING section blocks verbatim - move each into the matching `section === "..."` group:

```tsx
  return (
    <div style={styles.page}>
      <aside style={styles.nav}>
        <div className="label" style={{ padding: "0 10px", marginBottom: 12 }}>SETTINGS</div>
        {SECTIONS.map((s) => {
          const active = section === s.id;
          return (
            <button key={s.id} onClick={() => setSection(s.id)}
              style={{ ...styles.navItem, ...(active ? styles.navItemActive : {}) }}
              aria-current={active ? "page" : undefined}>
              <s.Icon /> <span>{s.label}</span>
            </button>
          );
        })}
      </aside>
      <div style={styles.content}>
        {section === "ai" && (
          <>
            {/* LocalAiSetup + LOCAL LLM ENDPOINTS + PROVIDER CHAIN + AI PROVIDER (LEGACY) + AWS BEDROCK panels, moved verbatim from the old left column */}
          </>
        )}
        {section === "keys" && (<>{/* API KEYS panel verbatim */}</>)}
        {section === "google" && (<>{/* GOOGLE SERVICES panel verbatim */}</>)}
        {section === "notion" && (<>{/* NOTION panel verbatim */}</>)}
        {section === "github" && (<>{/* GITHUB panel verbatim */}</>)}
        {section === "obsidian" && (<>{/* OBSIDIAN panel verbatim */}</>)}
        {section === "voice" && (<>{/* LANGUAGE & VOICE + VOICE + WAKE WORD panels verbatim */}</>)}
        {section === "assistant" && (<>{/* ASSISTANT panel verbatim */}</>)}
        {section === "appearance" && (<>{/* WALLPAPER MODE panel verbatim */}</>)}
        {section === "account" && (<>{/* UPGRADE panel verbatim */}</>)}
      </div>
    </div>
  );
```

CRITICAL: "verbatim" means copy each existing `<div className="panel" ...>...</div>` block (and the `<LocalAiSetup .../>` element) exactly as it is now into the group - do not alter any `onClick`, `value`, `onChange`, `checked`, or command call. Only their container changes.

- [ ] **Step 3: Update the styles object**

Replace `container`/`grid`/`column` in `styles` with the nav/content shell (keep every other style key - `panel`, `option`, `radio`, `hint`, `tokenInput`, `saveBtn`, `actionBtn`, `select`, `code`, `sectionTitle`, `statusText`, `progressShell`, `progressFill`, `privacyText`, `errorText` - unchanged):

```tsx
  page: { display: "flex", height: "100%", overflow: "hidden" },
  nav: { width: 210, flexShrink: 0, borderRight: "1px solid var(--border-primary)", background: "var(--surface)", backdropFilter: "blur(12px)", padding: "18px 12px", overflowY: "auto" },
  navItem: { display: "flex", alignItems: "center", gap: 11, width: "100%", textAlign: "left" as const, padding: "9px 11px", marginBottom: 2, borderRadius: 9, border: "1px solid transparent", background: "transparent", color: "var(--muted)", fontSize: 13, cursor: "pointer" },
  navItemActive: { background: "rgba(0,180,255,0.12)", color: "var(--text-primary)", boxShadow: "inset 0 0 0 1px var(--border-strong)" },
  content: { flex: 1, overflowY: "auto", padding: "24px 30px", display: "flex", flexDirection: "column" as const, gap: 12 },
```

- [ ] **Step 4: Verify build passes**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 5: Visual check (control preservation)**

Run `npm run tauri dev`, open Settings. Confirm the left section nav switches groups, and spot-check that controls still work:
  - AI Providers: add/remove a local endpoint; reorder provider chain; AWS save/clear.
  - API Keys: the .env code list shows.
  - Notion/GitHub/Obsidian: type a token, SAVE flips to SAVED.
  - Voice: toggle TTS, change language selects, wake-word download button state.
  - Assistant: briefing checkbox.
  - Appearance: wallpaper toggle.
  - Account: billing email + UPGRADE button.

---

### Task 7: Final verification + commit

- [ ] **Step 1: Full build**

Run: `npm run build`
Expected: PASS (tsc + vite).

- [ ] **Step 2: Grep for leftover glyphs**

Run: `grep -nE '"\[X\]"|"\[>\]"|">_"|"NEW"|"\[TH\]"' src/components/Sidebar.tsx src/components/CommandBar.tsx src/components/ChatPanel.tsx`
Expected: no matches (all replaced by icons).

- [ ] **Step 3: Commit (after user visual sign-off)**

```bash
git add package.json package-lock.json src/components/icons.tsx src/styles/global.css src/components/Sidebar.tsx src/components/CommandBar.tsx src/components/ChatPanel.tsx src/App.tsx src/pages/Settings.tsx docs/superpowers/specs/2026-07-11-ui-shell-refresh-design.md docs/superpowers/plans/2026-07-11-ui-shell-refresh.md
git commit -m "feat(ui): refresh app chrome with lucide icons and cleaner sidebar/settings"
```

---

## Self-Review

- **Spec coverage:** icons barrel (T1) ✓; tokens (T2) ✓; sidebar mapping + active state (T3) ✓; command box + chat panel icons (T4) ✓; top bar (T5) ✓; settings section nav (T6) ✓; verification (T7) ✓. Accessibility (aria-label/aria-current) covered in T3/T4/T5/T6.
- **Deviation from spec (documented):** `UpdateButton` and `WindowControls` are left unstyled - UpdateButton renders download progress and is rarely visible; traffic lights are already on-brand SVGs. Spec's "icon-ify update/background buttons" is honored only for the send-to-background button. `CalendarPage` nav label was `CAL`; tooltip is now the fuller `CALENDAR`.
- **Placeholder scan:** the `{/* ... verbatim */}` markers in Task 6 Step 2 are intentional move-instructions, not code placeholders - the source blocks already exist in the file and Step 2 says copy them exactly. No TBD/TODO.
- **Type consistency:** icon barrel exports match imports in T3/T4/T5/T6 (`KeyRound as Key`). `section` state is `string`; `SECTIONS[].id` are strings. Sidebar props unchanged.
