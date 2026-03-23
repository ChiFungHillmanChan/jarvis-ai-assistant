import { getCurrentWindow } from "@tauri-apps/api/window";

export default function WindowControls() {
  const appWindow = getCurrentWindow();

  return (
    <div className="no-drag" style={styles.controls}>
      <button onClick={() => appWindow.minimize()} style={styles.btn} title="Minimize">
        <span style={styles.icon}>--</span>
      </button>
      <button onClick={() => appWindow.toggleMaximize()} style={styles.btn} title="Maximize">
        <span style={styles.icon}>[]</span>
      </button>
      <button onClick={() => appWindow.close()} style={{ ...styles.btn, ...styles.closeBtn }} title="Close">
        <span style={styles.icon}>x</span>
      </button>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  controls: {
    display: "flex",
    gap: 6,
    alignItems: "center",
  },
  btn: {
    width: 14,
    height: 14,
    borderRadius: "50%",
    border: "1px solid rgba(0, 180, 255, 0.3)",
    background: "rgba(0, 180, 255, 0.08)",
    cursor: "pointer",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    padding: 0,
  },
  closeBtn: {
    borderColor: "rgba(255, 100, 100, 0.3)",
    background: "rgba(255, 100, 100, 0.08)",
  },
  icon: {
    fontSize: 7,
    color: "rgba(0, 180, 255, 0.6)",
    fontFamily: "var(--font-mono)",
    lineHeight: 1,
  },
};
