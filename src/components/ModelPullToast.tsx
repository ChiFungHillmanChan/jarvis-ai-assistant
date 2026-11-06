import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useUiLocale } from '../i18n';
import type { ModelPullProgress } from "../lib/types";

/** Ollama download progress. It keeps its own state: a pull sends an event for
 * every progress line, and only this toast needs to redraw for them. */
export default function ModelPullToast() {
  const { text } = useUiLocale();
  const [progress, setProgress] = useState<ModelPullProgress | null>(null);
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout>;
    const unlisten = listen<ModelPullProgress>('model-pull-progress', ({ payload }) => {
      clearTimeout(timer);
      // Byte counts change on every event; the toast shows only the model and percent.
      setProgress(previous => previous && previous.model === payload.model && previous.status === payload.status && previous.percent === payload.percent ? previous : payload);
      if (payload.status === 'complete' || payload.status === 'error') timer = setTimeout(() => setProgress(null), 3000);
    });
    return () => { clearTimeout(timer); void unlisten.then(fn => fn()).catch(() => {}); };
  }, []);
  if (!progress) return null;

  return (
    <div style={styles.container} className="animate-fade-in">
      <div style={styles.label}>
        {text("Downloading", "下載中")} {progress.model}... {progress.percent}%
      </div>
      <div style={styles.progressShell}>
        <div style={{ ...styles.progressFill, width: `${progress.percent}%` }} />
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: {
    position: "fixed",
    bottom: 16,
    right: 16,
    zIndex: 200,
    background: "rgba(6, 10, 20, 0.95)",
    border: "1px solid rgba(0, 180, 255, 0.2)",
    borderRadius: 10,
    padding: "10px 16px",
    minWidth: 240,
  },
  label: {
    color: "rgba(0, 180, 255, 0.7)",
    fontSize: 10,
    fontFamily: "var(--font-mono)",
    letterSpacing: 0.5,
    marginBottom: 6,
  },
  progressShell: {
    width: "100%",
    height: 5,
    borderRadius: 999,
    overflow: "hidden",
    background: "rgba(0, 180, 255, 0.08)",
    border: "1px solid rgba(0, 180, 255, 0.12)",
  },
  progressFill: {
    height: "100%",
    background: "linear-gradient(90deg, rgba(0, 180, 255, 0.65), rgba(96, 165, 250, 0.95))",
    transition: "width 200ms ease-out",
  },
};
