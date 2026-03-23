import { useState, useRef, useEffect } from "react";
import ChatMessageComponent from "./ChatMessage";
import { useChat } from "../hooks/useChat";

interface ChatPanelProps { isOpen: boolean; isFullScreen: boolean; onClose: () => void; onToggleFullScreen: () => void; }

export default function ChatPanel({ isOpen, isFullScreen, onClose, onToggleFullScreen }: ChatPanelProps) {
  const { messages, loading, error, send } = useChat();
  const [input, setInput] = useState("");
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => { messagesEndRef.current?.scrollIntoView({ behavior: "smooth" }); }, [messages]);
  useEffect(() => { if (isOpen) inputRef.current?.focus(); }, [isOpen]);

  if (!isOpen) return null;

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (input.trim()) { send(input); setInput(""); }
  }

  const panelStyle: React.CSSProperties = isFullScreen ? styles.fullScreen : styles.overlay;
  return (
    <div style={panelStyle}>
      <div style={styles.header}>
        <span className="system-text">JARVIS CHAT</span>
        <div style={styles.headerActions}>
          <button onClick={onToggleFullScreen} style={styles.headerBtn}>{isFullScreen ? "[-]" : "[+]"}</button>
          <button onClick={onClose} style={styles.headerBtn}>[X]</button>
        </div>
      </div>
      <div style={styles.messages}>
        {messages.map((msg, i) => <ChatMessageComponent key={msg.id ?? i} message={msg} />)}
        {loading && <div className="system-text animate-glow" style={{ padding: 8 }}>PROCESSING...</div>}
        {error && <div style={{ color: "var(--accent-urgent)", fontSize: 12, padding: 8 }}>{error}</div>}
        <div ref={messagesEndRef} />
      </div>
      <form onSubmit={handleSubmit} style={styles.inputForm}>
        <input ref={inputRef} type="text" value={input} onChange={(e) => setInput(e.target.value)} placeholder="Talk to JARVIS..." style={styles.input} />
      </form>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  overlay: { position: "fixed", top: 0, right: 0, width: 380, height: "100%", borderLeft: "1px solid rgba(0, 180, 255, 0.15)", background: "rgba(10, 14, 26, 0.97)", display: "flex", flexDirection: "column", zIndex: 100 },
  fullScreen: { position: "fixed", top: 0, left: 0, right: 0, bottom: 0, background: "rgba(10, 14, 26, 0.99)", display: "flex", flexDirection: "column", zIndex: 100 },
  header: { display: "flex", justifyContent: "space-between", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid rgba(0, 180, 255, 0.1)" },
  headerActions: { display: "flex", gap: 8 },
  headerBtn: { background: "transparent", border: "none", color: "rgba(0, 180, 255, 0.5)", fontFamily: "var(--font-mono)", fontSize: 11, cursor: "pointer" },
  messages: { flex: 1, overflowY: "auto", padding: 16 },
  inputForm: { padding: 12, borderTop: "1px solid rgba(0, 180, 255, 0.1)" },
  input: { width: "100%", background: "rgba(0, 180, 255, 0.03)", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 8, padding: "10px 14px", color: "rgba(0, 180, 255, 0.8)", fontSize: 13, fontFamily: "var(--font-sans)", outline: "none" },
};
