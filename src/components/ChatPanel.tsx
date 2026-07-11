import { useState, useRef, useEffect, useCallback, memo } from "react";
import ChatMessageComponent from "./ChatMessage";
import { useChatContext } from "../hooks/ChatContext";
import { Plus, Maximize2, X, Sparkles, ArrowUp } from "./icons";

interface ChatPanelProps {
  isOpen: boolean;
  onClose: () => void;
  onNavigateToChat: () => void;
}

export default memo(function ChatPanel({ isOpen, onClose, onNavigateToChat }: ChatPanelProps) {
  const { messages, loading, error, send, clearChat, currentStatus, streamingText } = useChatContext();
  const [input, setInput] = useState("");
  const [thinkHarder, setThinkHarder] = useState(false);
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
    if (input.trim()) {
      send(input, thinkHarder ? "premium" : "tools");
      setInput("");
      setThinkHarder(false);
    }
  }, [input, send, thinkHarder]);

  if (!isOpen) return null;

  return (
    <div style={styles.overlay} className="no-drag">
      <div style={styles.header}>
        <span className="system-text" style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Sparkles size={14} /> JARVIS
        </span>
        <div style={styles.headerActions}>
          <button onClick={clearChat} style={styles.iconBtn} title="New chat" aria-label="New chat"><Plus size={16} /></button>
          <button onClick={onNavigateToChat} style={styles.iconBtn} title="Open full view" aria-label="Open full view"><Maximize2 size={15} /></button>
          <button onClick={onClose} style={styles.iconBtn} title="Close" aria-label="Close"><X size={16} /></button>
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
        <div style={styles.inputBar}>
          <textarea
            ref={inputRef}
            value={input}
            onChange={(e) => {
              setInput(e.target.value);
              e.target.style.height = "auto";
              e.target.style.height = Math.min(e.target.scrollHeight, 150) + "px";
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                handleSubmit(e);
                if (inputRef.current) inputRef.current.style.height = "auto";
              }
            }}
            placeholder="Talk to JARVIS..."
            style={styles.input}
            rows={1}
          />
          <button
            type="button"
            onClick={() => setThinkHarder((v) => !v)}
            style={thinkHarder ? styles.thinkHarderButtonActive : styles.thinkHarderButton}
            title="Think harder (routes to Sonnet 4.6)"
            aria-pressed={thinkHarder}
          >
            <Sparkles size={12} /> Think harder
          </button>
          <button type="submit" style={styles.sendButton} disabled={!input.trim()} aria-label="Send">
            <ArrowUp size={16} />
          </button>
        </div>
      </form>
    </div>
  );
});

const styles: Record<string, React.CSSProperties> = {
  overlay: { position: "fixed", top: 0, right: 0, width: 440, height: "100%", borderLeft: "1px solid rgba(0, 180, 255, 0.15)", background: "rgba(10, 14, 26, 0.97)", display: "flex", flexDirection: "column", zIndex: 100, userSelect: "text" as const },
  header: { display: "flex", justifyContent: "space-between", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid rgba(0, 180, 255, 0.1)" },
  headerActions: { display: "flex", gap: 4 },
  iconBtn: { display: "flex", alignItems: "center", justifyContent: "center", width: 28, height: 28, borderRadius: 7, background: "transparent", border: "1px solid transparent", color: "var(--muted)", cursor: "pointer" },
  messages: { flex: 1, overflowY: "auto", padding: 16, userSelect: "text" as const, cursor: "text" },
  inputForm: { padding: 12, borderTop: "1px solid rgba(0, 180, 255, 0.1)" },
  inputBar: { display: "flex", alignItems: "flex-end", gap: 8 },
  input: { flex: 1, background: "rgba(0, 180, 255, 0.04)", border: "1px solid rgba(0, 180, 255, 0.18)", borderRadius: 12, padding: "10px 14px", color: "rgba(0, 180, 255, 0.8)", fontSize: 13, fontFamily: "var(--font-sans)", outline: "none", resize: "none" as const, overflow: "hidden", lineHeight: 1.5, boxSizing: "border-box" as const },
  sendButton: { width: 34, height: 34, borderRadius: "50%", background: "rgba(0, 180, 255, 0.12)", border: "1px solid rgba(0, 180, 255, 0.25)", color: "rgb(0, 180, 255)", display: "flex", alignItems: "center", justifyContent: "center", cursor: "pointer", flexShrink: 0, boxShadow: "0 0 8px rgba(0, 180, 255, 0.08)" },
  thinkHarderButton: { display: "flex", alignItems: "center", gap: 6, height: 34, padding: "0 10px", borderRadius: 8, background: "transparent", border: "1px solid rgba(0, 180, 255, 0.18)", color: "rgba(0, 180, 255, 0.4)", fontFamily: "var(--font-mono)", fontSize: 10, cursor: "pointer", flexShrink: 0, whiteSpace: "nowrap" as const },
  thinkHarderButtonActive: { display: "flex", alignItems: "center", gap: 6, height: 34, padding: "0 10px", borderRadius: 8, background: "rgba(0, 180, 255, 0.12)", border: "1px solid rgba(0, 180, 255, 0.5)", color: "rgba(0, 180, 255, 0.9)", fontFamily: "var(--font-mono)", fontSize: 10, cursor: "pointer", flexShrink: 0, whiteSpace: "nowrap" as const, boxShadow: "0 0 8px rgba(0, 180, 255, 0.2)" },
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
