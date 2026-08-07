import { Component } from "react";
import type { ErrorInfo, ReactNode } from "react";

interface ErrorBoundaryProps {
  children: ReactNode;
}

interface ErrorBoundaryState {
  error: Error | null;
}

// The window runs with decorations disabled, so an uncaught render error would
// unmount the title bar and window controls along with everything else. This
// keeps a reachable recovery panel on screen instead of a blank window.
export default class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Render error:", error, info.componentStack);
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;

    return (
      <div data-tauri-drag-region style={styles.container}>
        <div style={styles.panel}>
          <div style={styles.label}>SYSTEM FAULT</div>
          <div style={styles.message}>{error.message || String(error)}</div>
          <button
            type="button"
            style={styles.button}
            onClick={() => window.location.reload()}
          >
            RELOAD
          </button>
        </div>
      </div>
    );
  }
}

const styles: Record<string, React.CSSProperties> = {
  container: {
    height: "100%",
    minHeight: "100%",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    padding: 24,
    background: "var(--bg-primary)",
  },
  panel: {
    maxWidth: 460,
    width: "100%",
    display: "flex",
    flexDirection: "column",
    gap: 12,
    padding: 20,
    borderRadius: 12,
    border: "1px solid rgba(0, 180, 255, 0.25)",
    background: "rgba(0, 180, 255, 0.04)",
  },
  label: {
    fontFamily: "var(--font-mono)",
    fontSize: 10,
    letterSpacing: 3,
    color: "rgba(0, 180, 255, 0.5)",
  },
  message: {
    fontFamily: "var(--font-mono)",
    fontSize: 12,
    lineHeight: 1.6,
    color: "rgba(0, 180, 255, 0.8)",
    wordBreak: "break-word",
  },
  button: {
    alignSelf: "flex-start",
    background: "rgba(0, 180, 255, 0.12)",
    border: "1px solid rgba(0, 180, 255, 0.3)",
    borderRadius: 8,
    padding: "6px 14px",
    color: "rgba(0, 180, 255, 0.85)",
    fontFamily: "var(--font-mono)",
    fontSize: 11,
    letterSpacing: 1,
    cursor: "pointer",
  },
};
