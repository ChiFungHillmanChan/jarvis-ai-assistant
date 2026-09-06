import { memo } from "react";
import {
  LayoutDashboard, MessageSquare, Mail, Calendar, Clock, Settings,
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
