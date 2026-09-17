import { Component } from "react";
import type { ErrorInfo, ReactNode } from "react";
import { getUiLocale } from "../i18n";

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
          <div style={styles.label}>{getUiLocale() === "zh-HK" ? "未能載入此工作區" : "This workspace could not load"}</div>
          <div style={styles.message}>{error.message || String(error)}</div>
          <button
            type="button"
            style={styles.button}
            onClick={() => this.setState({ error: null })}
          >
            {getUiLocale() === "zh-HK" ? "重試" : "Try again"}
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
    border: "1px solid var(--border-primary)",
    background: "var(--surface)",
  },
  label: {
    fontFamily: "var(--font-sans)",
    fontSize: 17,
    fontWeight: 600,
    color: "var(--text-primary)",
  },
  message: {
    fontFamily: "var(--font-sans)",
    fontSize: 12,
    lineHeight: 1.6,
    color: "var(--text-secondary)",
    wordBreak: "break-word",
  },
  button: {
    alignSelf: "flex-start",
    background: "var(--surface-solid)",
    border: "1px solid var(--border-strong)",
    borderRadius: 8,
    padding: "6px 14px",
    color: "var(--text-primary)",
    fontFamily: "var(--font-sans)",
    fontSize: 13,
    cursor: "pointer",
  },
};
