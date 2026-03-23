import { useState, useEffect, useCallback } from "react";

interface ToastMessage {
  id: number;
  text: string;
  type: "error" | "success" | "info";
}

let toastId = 0;
let addToastFn: ((text: string, type: "error" | "success" | "info") => void) | null = null;

export function showToast(text: string, type: "error" | "success" | "info" = "info") {
  addToastFn?.(text, type);
}

export default function ToastContainer() {
  const [toasts, setToasts] = useState<ToastMessage[]>([]);

  const addToast = useCallback((text: string, type: "error" | "success" | "info") => {
    const id = ++toastId;
    setToasts(prev => [...prev, { id, text, type }]);
    setTimeout(() => setToasts(prev => prev.filter(t => t.id !== id)), 4000);
  }, []);

  useEffect(() => { addToastFn = addToast; return () => { addToastFn = null; }; }, [addToast]);

  if (toasts.length === 0) return null;

  return (
    <div style={styles.container}>
      {toasts.map(toast => {
        const color = toast.type === "error" ? "rgba(255, 100, 100, 0.8)"
          : toast.type === "success" ? "rgba(16, 185, 129, 0.8)"
          : "rgba(0, 180, 255, 0.8)";
        const bg = toast.type === "error" ? "rgba(255, 100, 100, 0.06)"
          : toast.type === "success" ? "rgba(16, 185, 129, 0.06)"
          : "rgba(0, 180, 255, 0.06)";
        return (
          <div key={toast.id} className="animate-fade-in" style={{ ...styles.toast, borderColor: color, background: bg }}>
            <span style={{ color, fontSize: 12 }}>{toast.text}</span>
          </div>
        );
      })}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: { position: "fixed", top: 44, right: 16, zIndex: 300, display: "flex", flexDirection: "column", gap: 8, maxWidth: 350 },
  toast: { padding: "10px 16px", borderRadius: 8, border: "1px solid", backdropFilter: "blur(8px)" },
};
