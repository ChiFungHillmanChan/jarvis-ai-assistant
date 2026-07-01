import { getCurrentWindow } from "@tauri-apps/api/window";

export default function WindowControls() {
  const appWindow = getCurrentWindow();

  return (
    <div data-tauri-drag-region="false" style={styles.controls}>
      <button
        onClick={() => appWindow.minimize()}
        style={{ ...styles.btn, ...styles.minBtn }}
        title="Minimize"
        aria-label="Minimize window"
      >
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <line x1="2" y1="5" x2="8" y2="5" stroke="rgba(0, 0, 0, 0.55)" strokeWidth="1.2" strokeLinecap="round" />
        </svg>
      </button>
      <button
        onClick={() => appWindow.toggleMaximize()}
        style={{ ...styles.btn, ...styles.maxBtn }}
        title="Maximize"
        aria-label="Maximize window"
      >
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <rect x="2.2" y="2.2" width="5.6" height="5.6" rx="1" fill="none" stroke="rgba(0, 0, 0, 0.55)" strokeWidth="1.2" />
        </svg>
      </button>
      <button
        onClick={() => appWindow.close()}
        style={{ ...styles.btn, ...styles.closeBtn }}
        title="Close"
        aria-label="Close window"
      >
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <line x1="2.5" y1="2.5" x2="7.5" y2="7.5" stroke="rgba(0, 0, 0, 0.7)" strokeWidth="1.4" strokeLinecap="round" />
          <line x1="7.5" y1="2.5" x2="2.5" y2="7.5" stroke="rgba(0, 0, 0, 0.7)" strokeWidth="1.4" strokeLinecap="round" />
        </svg>
      </button>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  controls: {
    display: "flex",
    gap: 8,
    alignItems: "center",
    WebkitAppRegion: "no-drag",
  } as React.CSSProperties,
  btn: {
    width: 16,
    height: 16,
    borderRadius: "50%",
    border: "1px solid rgba(0, 0, 0, 0.15)",
    cursor: "pointer",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    padding: 0,
    transition: "filter 120ms ease-out",
  },
  minBtn: {
    background: "#febc2e",
  },
  maxBtn: {
    background: "#28c840",
  },
  closeBtn: {
    background: "#ff5f57",
  },
};
