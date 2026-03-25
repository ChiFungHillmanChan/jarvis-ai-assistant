import { memo } from "react";
interface SidebarProps {
  activeView: string;
  onNavigate: (view: string) => void;
  onChatToggle: () => void;
}

const navItems = [
  { id: "home", label: "HOME", icon: "H" },
  { id: "email", label: "MAIL", icon: "M" },
  { id: "calendar", label: "CAL", icon: "C" },
  { id: "github", label: "GIT", icon: "G" },
  { id: "notion", label: "NOT", icon: "N" },
  { id: "cron", label: "CRON", icon: "T" },
  { id: "settings", label: "SET", icon: "S" },
];

export default memo(function Sidebar({ activeView, onNavigate, onChatToggle }: SidebarProps) {
  return (
    <div style={styles.container}>
      <div style={styles.logo}><span style={styles.logoText}>J</span></div>
      <nav style={styles.nav}>
        {navItems.map((item) => (
          <button key={item.id} onClick={() => onNavigate(item.id)}
            style={{ ...styles.navButton, ...(activeView === item.id ? styles.navButtonActive : {}) }}
            title={item.label}>
            <span style={styles.navIcon}>{item.icon}</span>
          </button>
        ))}
      </nav>
      <button onClick={onChatToggle} style={styles.chatButton} title="Chat (Cmd+K)">
        <span style={styles.navIcon}>&gt;_</span>
      </button>
    </div>
  );
});

const styles: Record<string, React.CSSProperties> = {
  container: { width: 56, height: "100%", display: "flex", flexDirection: "column", alignItems: "center", padding: "12px 0", borderRight: "1px solid rgba(0, 180, 255, 0.1)", background: "rgba(10, 14, 26, 0.6)", backdropFilter: "blur(12px)", gap: 4 },
  logo: { width: 32, height: 32, border: "1px solid rgba(0, 180, 255, 0.4)", borderRadius: 8, display: "flex", alignItems: "center", justifyContent: "center", marginBottom: 16 },
  logoText: { color: "rgba(0, 180, 255, 0.9)", fontFamily: "var(--font-mono)", fontSize: 14, fontWeight: 500 },
  nav: { display: "flex", flexDirection: "column", gap: 4, flex: 1 },
  navButton: { width: 36, height: 36, border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 8, background: "transparent", cursor: "pointer", display: "flex", alignItems: "center", justifyContent: "center", transition: "all 0.2s ease" },
  navButtonActive: { background: "rgba(0, 180, 255, 0.1)", borderColor: "rgba(0, 180, 255, 0.4)" },
  navIcon: { color: "rgba(0, 180, 255, 0.6)", fontFamily: "var(--font-mono)", fontSize: 10, fontWeight: 500 },
  chatButton: { width: 36, height: 36, border: "1px solid rgba(0, 180, 255, 0.25)", borderRadius: "50%", background: "transparent", cursor: "pointer", display: "flex", alignItems: "center", justifyContent: "center", marginTop: "auto" },
};
