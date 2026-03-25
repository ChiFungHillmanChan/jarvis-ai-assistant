import { useState, useRef, useEffect, useCallback, memo } from "react";
import ChatMessageComponent from "./ChatMessage";
import { useChat } from "../hooks/useChat";

interface ChatPanelProps { isOpen: boolean; isFullScreen: boolean; onClose: () => void; onToggleFullScreen: () => void; }

export default memo(function ChatPanel({ isOpen, isFullScreen, onClose, onToggleFullScreen }: ChatPanelProps) {
  const { messages, loading, error, send, clearChat, currentStatus, streamingText } = useChat();
  const [input, setInput] = useState("");
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const scrollRafRef = useRef<number | null>(null);

  // Scroll on new messages only (not every streaming token)
  useEffect(() => { messagesEndRef.current?.scrollIntoView({ behavior: "smooth" }); }, [messages]);

  // Throttled scroll during streaming — at most once per animation frame
  useEffect(() => {
    if (!streamingText) return;
    if (scrollRafRef.current) return;
    scrollRafRef.current = requestAnimationFrame(() => {
      messagesEndRef.current?.scrollIntoView({ behavior: "auto" });
      scrollRafRef.current = null;
    });
    return () => { if (scrollRafRef.current) { cancelAnimationFrame(scrollRafRef.current); scrollRafRef.current = null; } };
  }, [streamingText]);
  useEffect(() => { if (isOpen) inputRef.current?.focus(); }, [isOpen]);

  const handleSubmit = useCallback((e: React.FormEvent) => {
    e.preventDefault();
    if (input.trim()) { send(input); setInput(""); }
  }, [input, send]);

  if (!isOpen) return null;

  const panelStyle: React.CSSProperties = isFullScreen ? styles.fullScreen : styles.overlay;
  return (
    <div style={panelStyle} className="no-drag">
      <div style={styles.header}>
        <span className="system-text">JARVIS CHAT</span>
        <div style={styles.headerActions}>
          <button onClick={clearChat} style={styles.headerBtn} title="New chat">NEW</button>
          <button onClick={onToggleFullScreen} style={styles.headerBtn}>{isFullScreen ? "[-]" : "[+]"}</button>
          <button onClick={onClose} style={styles.headerBtn}>[X]</button>
        </div>
      </div>
      <div style={styles.messages}>
        {messages.length === 0 && (
          <div style={styles.empty}>Start a conversation with JARVIS.</div>
        )}
        {messages.map((msg, i) => <ChatMessageComponent key={msg.id ?? i} message={msg} />)}
        {loading && (
          <div style={styles.liveResponse}>
            {/* Status bar: always visible during loading */}
            <div style={styles.statusBar}>
              <span style={streamingText ? styles.statusDotActive : (currentStatus ? styles.statusDotCyan : styles.statusDotAmber)} />
              <span style={styles.statusText}>
                {streamingText ? "RESPONDING" : currentStatus || "PROCESSING..."}
              </span>
            </div>
            {/* Streaming text: grows as tokens arrive */}
            {streamingText && (
              <div style={styles.streamingBubble}>
                {streamingText}
                <span style={styles.cursor}>|</span>
              </div>
            )}
          </div>
        )}
        {error && <div style={{ color: "var(--accent-urgent)", fontSize: 12, padding: 8 }}>{error}</div>}
        <div ref={messagesEndRef} />
      </div>
      <form onSubmit={handleSubmit} style={styles.inputForm}>
        <textarea
          ref={inputRef}
          value={input}
          onChange={(e) => {
            setInput(e.target.value);
            // Auto-grow: reset height then set to scrollHeight
            e.target.style.height = "auto";
            e.target.style.height = Math.min(e.target.scrollHeight, 150) + "px";
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              handleSubmit(e);
              // Reset height after send
              if (inputRef.current) inputRef.current.style.height = "auto";
            }
          }}
          placeholder="Talk to JARVIS..."
          style={styles.input}
          rows={1}
        />
      </form>
    </div>
  );
});

const styles: Record<string, React.CSSProperties> = {
  overlay: { position: "fixed", top: 0, right: 0, width: 380, height: "100%", borderLeft: "1px solid rgba(0, 180, 255, 0.15)", background: "rgba(10, 14, 26, 0.97)", display: "flex", flexDirection: "column", zIndex: 100, userSelect: "text" as const },
  fullScreen: { position: "fixed", top: 0, left: 0, right: 0, bottom: 0, background: "rgba(10, 14, 26, 0.99)", display: "flex", flexDirection: "column", zIndex: 100, userSelect: "text" as const },
  header: { display: "flex", justifyContent: "space-between", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid rgba(0, 180, 255, 0.1)" },
  headerActions: { display: "flex", gap: 8 },
  headerBtn: { background: "transparent", border: "none", color: "rgba(0, 180, 255, 0.5)", fontFamily: "var(--font-mono)", fontSize: 11, cursor: "pointer" },
  messages: { flex: 1, overflowY: "auto", padding: 16, userSelect: "text" as const, cursor: "text" },
  inputForm: { padding: 12, borderTop: "1px solid rgba(0, 180, 255, 0.1)" },
  input: { width: "100%", background: "rgba(0, 180, 255, 0.03)", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 8, padding: "10px 14px", color: "rgba(0, 180, 255, 0.8)", fontSize: 13, fontFamily: "var(--font-sans)", outline: "none", resize: "none" as const, overflow: "hidden", lineHeight: 1.5, boxSizing: "border-box" as const },
  empty: { color: "rgba(0, 180, 255, 0.2)", fontSize: 12, fontStyle: "italic", textAlign: "center" as const, padding: 40 },
  liveResponse: { margin: "8px 0", background: "rgba(0, 180, 255, 0.03)", border: "1px solid rgba(0, 180, 255, 0.1)", borderRadius: 8, overflow: "hidden" },
  statusBar: { display: "flex", alignItems: "center", gap: 8, padding: "8px 12px", borderBottom: "1px solid rgba(0, 180, 255, 0.06)", background: "rgba(0, 180, 255, 0.02)" },
  statusText: { fontSize: 11, color: "rgba(0, 180, 255, 0.6)", fontFamily: "var(--font-mono)", letterSpacing: 1.5 },
  statusDotAmber: { display: "inline-block", width: 8, height: 8, borderRadius: "50%", background: "rgba(255, 180, 0, 0.7)", animation: "glow-pulse 1s ease-in-out infinite", flexShrink: 0 },
  statusDotCyan: { display: "inline-block", width: 8, height: 8, borderRadius: "50%", background: "rgba(0, 180, 255, 0.6)", animation: "glow-pulse 1.5s ease-in-out infinite", flexShrink: 0 },
  statusDotActive: { display: "inline-block", width: 8, height: 8, borderRadius: "50%", background: "rgba(16, 185, 129, 0.7)", animation: "glow-pulse 1s ease-in-out infinite", flexShrink: 0 },
  streamingBubble: { fontSize: 13, color: "rgba(0, 180, 255, 0.85)", fontFamily: "var(--font-sans)", lineHeight: 1.6, padding: "12px 14px", whiteSpace: "pre-wrap" as const },
  cursor: { color: "rgba(0, 180, 255, 0.5)", animation: "blink 1s step-end infinite" },
};
