import { memo } from "react";
import type { ChatMessage as ChatMessageType } from "../lib/types";
interface ChatMessageProps { message: ChatMessageType; }
export default memo(function ChatMessage({ message }: ChatMessageProps) {
  const isUser = message.role === "user";
  return (
    <div style={{ ...styles.container, alignItems: isUser ? "flex-end" : "flex-start" }}>
      <div style={styles.label}>{isUser ? "YOU" : "JARVIS"}</div>
      <div style={{ ...styles.bubble, borderColor: isUser ? "rgba(0, 180, 255, 0.2)" : "rgba(0, 180, 255, 0.12)", background: isUser ? "rgba(0, 180, 255, 0.06)" : "rgba(0, 180, 255, 0.02)" }}>
        {message.content}
      </div>
    </div>
  );
})
const styles: Record<string, React.CSSProperties> = {
  container: { display: "flex", flexDirection: "column", marginBottom: 14, maxWidth: "85%" },
  label: { color: "rgba(0, 180, 255, 0.4)", fontSize: 9, fontFamily: "var(--font-mono)", letterSpacing: 1.5, marginBottom: 4 },
  bubble: { border: "1px solid", borderRadius: 8, padding: "10px 14px", color: "rgba(0, 180, 255, 0.8)", fontSize: 13, lineHeight: 1.5 },
};
