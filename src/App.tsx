import { useState, useCallback, useEffect, useRef, useMemo, lazy, Suspense } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import Sidebar from "./components/Sidebar";
import ChatPanel from "./components/ChatPanel";
import CommandBar from "./components/CommandBar";
import Dashboard from "./pages/Dashboard";
const Settings = lazy(() => import("./pages/Settings"));
const CronDashboard = lazy(() => import("./pages/CronDashboard"));
const EmailPage = lazy(() => import("./pages/EmailPage"));
const CalendarPage = lazy(() => import("./pages/CalendarPage"));
const GitHubPage = lazy(() => import("./pages/GitHubPage"));
const NotionPage = lazy(() => import("./pages/NotionPage"));
import { useKeyboard } from "./hooks/useKeyboard";
import VoiceIndicator from "./components/VoiceIndicator";
import { useVoiceState } from "./hooks/useVoiceState";
import ToastContainer from "./components/Toast";
import WindowControls from "./components/WindowControls";
import JarvisScene from "./components/3d/JarvisScene";
import AssistantHud from "./components/AssistantHud";
import { ChatProvider, useChatContext } from "./hooks/ChatContext";
import ChatMessageComponent from "./components/ChatMessage";
import ModelPullToast from "./components/ModelPullToast";
import type { AssistantPhase, ChatStatusPayload, ChatThinkingPayload, ChatTokenPayload, ChatStatePayload, ModelPullProgress } from "./lib/types";

function ChatFullView() {
  const { messages, loading, error, send, clearChat, streamingText } = useChatContext();
  const [input, setInput] = useState("");
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => { messagesEndRef.current?.scrollIntoView({ behavior: "smooth" }); }, [messages]);
  useEffect(() => { inputRef.current?.focus(); }, []);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (input.trim()) { send(input); setInput(""); }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", padding: 24 }}>
      <div style={{ display: "flex", justifyContent: "space-between", marginBottom: 16 }}>
        <span className="system-text">JARVIS CHAT</span>
        <button onClick={clearChat} style={{ background: "transparent", border: "none", color: "rgba(0, 180, 255, 0.5)", fontFamily: "var(--font-mono)", fontSize: 11, cursor: "pointer" }}>NEW</button>
      </div>
      <div style={{ flex: 1, overflowY: "auto", maxWidth: 700 }}>
        {messages.map((msg, i) => <ChatMessageComponent key={msg.id ?? i} message={msg} />)}
        {loading && streamingText && (
          <div style={{ padding: "12px 14px", color: "rgba(0, 180, 255, 0.85)", fontSize: 13, lineHeight: 1.6, whiteSpace: "pre-wrap" as const }}>
            {streamingText}<span style={{ color: "rgba(0, 180, 255, 0.5)", animation: "blink 1s step-end infinite" }}>|</span>
          </div>
        )}
        {error && <div style={{ color: "var(--accent-urgent)", fontSize: 12, padding: 8 }}>{error}</div>}
        <div ref={messagesEndRef} />
      </div>
      <form onSubmit={handleSubmit} style={{ maxWidth: 700, paddingTop: 12, borderTop: "1px solid rgba(0, 180, 255, 0.08)" }}>
        <div style={{ display: "flex", alignItems: "flex-end", gap: 8 }}>
          <textarea ref={inputRef} value={input} onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleSubmit(e); } }}
            placeholder="Talk to JARVIS..." rows={1}
            style={{ flex: 1, background: "rgba(0, 180, 255, 0.04)", border: "1px solid rgba(0, 180, 255, 0.18)", borderRadius: 12, padding: "10px 14px", color: "rgba(0, 180, 255, 0.8)", fontSize: 13, fontFamily: "var(--font-sans)", outline: "none", resize: "none" as const, overflow: "hidden", lineHeight: 1.5, boxSizing: "border-box" as const }} />
          <button type="submit" style={{ width: 34, height: 34, borderRadius: "50%", background: "rgba(0, 180, 255, 0.12)", border: "1px solid rgba(0, 180, 255, 0.25)", display: "flex", alignItems: "center", justifyContent: "center", cursor: "pointer", flexShrink: 0 }}>
            <svg width="14" height="14" viewBox="0 0 14 14"><path d="M3 7h8M8 4l3 3-3 3" fill="none" stroke="rgba(0, 180, 255, 0.7)" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" /></svg>
          </button>
        </div>
      </form>
    </div>
  );
}

export default function App() {
  const [activeView, setActiveView] = useState("home");
  const [chatOpen, setChatOpen] = useState(false);
  const [wallpaperActive, setWallpaperActive] = useState(false);
  const [wallpaperRaised, setWallpaperRaised] = useState(false);
  const [aiState, setAiState] = useState<"idle" | "thinking" | "speaking">("idle");
  const [pageTransition, setPageTransition] = useState(false);
  const [assistantPhase, setAssistantPhase] = useState<AssistantPhase>("idle");
  const [assistantStatus, setAssistantStatus] = useState<string | null>(null);
  const [assistantThinking, setAssistantThinking] = useState("");
  const [assistantResponsePreview, setAssistantResponsePreview] = useState("");
  const [voiceRequestActive, setVoiceRequestActive] = useState(false);
  const ttsAmplitudeRef = useRef(0);
  const micAmplitudeRef = useRef(0);
  const [pendingToolCall, setPendingToolCall] = useState<string | null>(null);
  const [modelPullProgress, setModelPullProgress] = useState<ModelPullProgress | null>(null);
  const prevView = useRef(activeView);
  const hudResetTimer = useRef<number | null>(null);
  const aiStateRef = useRef(aiState);
  const assistantThinkingRef = useRef(assistantThinking);
  const hudTokenBuffer = useRef("");
  const hudThinkingBuffer = useRef("");
  const hudRafId = useRef<number | null>(null);

  const toggleChat = useCallback(() => { setChatOpen((prev) => !prev); }, []);
  const closeChat = useCallback(() => { setChatOpen(false); }, []);
  const handleToolCallConsumed = useCallback(() => setPendingToolCall(null), []);

  const { state: voiceState, startVoice, stopVoice } = useVoiceState();

  useEffect(() => {
    aiStateRef.current = aiState;
  }, [aiState]);

  useEffect(() => {
    assistantThinkingRef.current = assistantThinking;
  }, [assistantThinking]);

  useEffect(() => {
    if (voiceState === "Listening" || voiceState === "WakeWordListening") {
      setAssistantPhase("listening");
      setAssistantStatus("Listening...");
    }
  }, [voiceState]);

  useEffect(() => {
    invoke<boolean>("get_wallpaper_status").then(setWallpaperActive).catch(() => {});
    invoke<boolean>("is_wallpaper_raised").then(setWallpaperRaised).catch(() => {});

    const unlistenStatus = listen<boolean>("wallpaper-status", (event) => {
      setWallpaperActive(event.payload);
      if (!event.payload) setWallpaperRaised(false);
    });
    const unlistenRaised = listen<boolean>("wallpaper-raised", (event) => {
      setWallpaperRaised(event.payload);
    });
    return () => {
      unlistenStatus.then((fn) => fn());
      unlistenRaised.then((fn) => fn());
    };
  }, []);

  // Listen for AI chat state changes
  useEffect(() => {
    const unlistenAi = listen<ChatStatePayload>("chat-state", (event) => {
      if (hudResetTimer.current) {
        window.clearTimeout(hudResetTimer.current);
        hudResetTimer.current = null;
      }
      setAiState(event.payload.state);
      if (event.payload.state === "speaking") {
        setAssistantPhase("speaking");
        setAssistantStatus((prev) => prev || "Speaking...");
      } else if (event.payload.state === "thinking") {
        setAssistantThinking("");
        setAssistantResponsePreview("");
        setAssistantPhase((prev) => (prev === "idle" ? "thinking" : prev));
      } else {
        hudResetTimer.current = window.setTimeout(() => {
          setAssistantPhase("idle");
          setAssistantStatus(null);
          setAssistantThinking("");
          setAssistantResponsePreview("");
        }, 900);
      }
    });
    return () => {
      if (hudResetTimer.current) {
        window.clearTimeout(hudResetTimer.current);
      }
      unlistenAi.then((fn) => fn());
    };
  }, []);

  // Listen for TTS amplitude, mic amplitude, and tool call events
  useEffect(() => {
    const appendTail = (prev: string, next: string, max: number) => {
      const merged = `${prev}${next}`;
      return merged.length <= max ? merged : merged.slice(-max);
    };

    const unlistenAmp = listen<{ amplitude: number }>("tts-amplitude", (event) => {
      // Write to ref instead of state -- JarvisScene reads via ref, no re-render cascade
      ttsAmplitudeRef.current = event.payload.amplitude;
    });
    const unlistenMic = listen<{ amplitude: number }>("mic-amplitude", (event) => {
      micAmplitudeRef.current = event.payload.amplitude;
    });
    const unlistenTool = listen<{ tool_name: string }>("chat-tool-call", (event) => {
      setPendingToolCall(event.payload.tool_name);
    });
    const unlistenStatus = listen<ChatStatusPayload>("chat-status", (event) => {
      setAssistantStatus(event.payload.status);
      if (event.payload.phase) {
        setAssistantPhase(event.payload.phase);
      }
      if (event.payload.phase === "responding" && assistantThinkingRef.current) {
        setAssistantThinking("");
      }
    });
    const flushHudBuffers = () => {
      if (hudTokenBuffer.current) {
        const tokens = hudTokenBuffer.current;
        hudTokenBuffer.current = "";
        setAssistantResponsePreview((prev) => appendTail(prev, tokens, 260));
      }
      if (hudThinkingBuffer.current) {
        const text = hudThinkingBuffer.current;
        hudThinkingBuffer.current = "";
        setAssistantThinking((prev) => appendTail(prev, text, 220));
      }
      hudRafId.current = null;
    };
    const scheduleHudFlush = () => {
      if (!hudRafId.current) {
        hudRafId.current = requestAnimationFrame(flushHudBuffers);
      }
    };
    const unlistenToken = listen<ChatTokenPayload>("chat-token", (event) => {
      if (event.payload.done) return;
      hudTokenBuffer.current += event.payload.token;
      scheduleHudFlush();
    });
    const unlistenThinking = listen<ChatThinkingPayload>("chat-thinking", (event) => {
      if (event.payload.done) return;
      hudThinkingBuffer.current += event.payload.text;
      scheduleHudFlush();
    });
    const unlistenVoiceRequest = listen<{ active: boolean }>("chat-voice-active", (event) => {
      setVoiceRequestActive(event.payload.active);
      if (!event.payload.active && aiStateRef.current === "idle") {
        setAssistantPhase("idle");
      } else if (event.payload.active) {
        setAssistantThinking("");
        setAssistantResponsePreview("");
      }
    });
    return () => {
      if (hudRafId.current) cancelAnimationFrame(hudRafId.current);
      unlistenAmp.then((fn) => fn());
      unlistenMic.then((fn) => fn());
      unlistenTool.then((fn) => fn());
      unlistenStatus.then((fn) => fn());
      unlistenToken.then((fn) => fn());
      unlistenThinking.then((fn) => fn());
      unlistenVoiceRequest.then((fn) => fn());
    };
  }, []);

  // Listen for model pull progress (for background toast)
  useEffect(() => {
    const unlisten = listen<ModelPullProgress>("model-pull-progress", (event) => {
      const p = event.payload;
      if (p.status === "complete" || p.status === "error") {
        setModelPullProgress(p);
        setTimeout(() => setModelPullProgress(null), 3000);
      } else {
        setModelPullProgress(p);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // Page transition pulse
  useEffect(() => {
    if (prevView.current !== activeView) {
      prevView.current = activeView;
      setPageTransition(true);
      const timer = setTimeout(() => setPageTransition(false), 250);
      return () => clearTimeout(timer);
    }
  }, [activeView]);

  const activityLevel = useMemo((): "idle" | "listening" | "processing" | "active" => {
    if (pageTransition) return "processing";
    if (aiState === "speaking") return "active";
    if (aiState === "thinking") return "processing";
    if (voiceState === "Speaking" || voiceState === "WakeWordSpeaking") return "active";
    if (voiceState === "Processing" || voiceState === "WakeWordDetected" || voiceState === "WakeWordProcessing" || (typeof voiceState === "object" && "ModelDownloading" in voiceState)) return "processing";
    if (voiceState === "Listening" || voiceState === "WakeWordListening") return "listening";
    return "idle";
  }, [pageTransition, aiState, voiceState]);

  const handleLowerToBackground = useCallback(async () => {
    await invoke("lower_wallpaper");
    setWallpaperRaised(false);
  }, []);

  const shortcuts = useMemo(() => ({
    "cmd+k": toggleChat,
    escape: () => {
      if (wallpaperActive && wallpaperRaised) {
        void handleLowerToBackground();
      } else {
        closeChat();
      }
    },
    "cmd+shift+j": () => {
      if (voiceState === "Listening") { stopVoice(); }
      else if (
        voiceState === "Idle" ||
        voiceState === "WakeWordListening" ||
        (typeof voiceState === "object" && "Error" in voiceState)
      ) { startVoice(); }
    },
  }), [toggleChat, closeChat, wallpaperActive, wallpaperRaised, handleLowerToBackground, voiceState, startVoice, stopVoice]);

  useKeyboard(shortcuts);

  function renderView() {
    switch (activeView) {
      case "chat": return <ChatFullView />;
      case "email": return <EmailPage />;
      case "calendar": return <CalendarPage />;
      case "github": return <GitHubPage />;
      case "notion": return <NotionPage />;
      case "cron": return <CronDashboard />;
      case "settings": return <Settings />;
      case "home": default: return <Dashboard />;
    }
  }

  return (
    <ChatProvider>
    <div style={styles.root}>
      <JarvisScene activityLevel={activityLevel} assistantPhase={assistantPhase} ttsAmplitudeRef={ttsAmplitudeRef} micAmplitudeRef={micAmplitudeRef} pendingToolCall={pendingToolCall} onToolCallConsumed={handleToolCallConsumed} />

      <div style={styles.uiLayer}>
        <div className="drag-region" style={styles.titleBar} onMouseDown={(e) => { if ((e.target as HTMLElement).closest('.no-drag')) return; getCurrentWindow().startDragging(); }}>
          <span style={styles.titleText}>JARVIS{wallpaperActive ? " [WALLPAPER]" : ""}</span>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            {wallpaperActive && wallpaperRaised && (
              <button
                className="no-drag"
                onClick={handleLowerToBackground}
                style={styles.lowerBtn}
              >
                SEND TO BACKGROUND
              </button>
            )}
            <WindowControls />
          </div>
        </div>
        <div style={styles.layout}>
          <Sidebar activeView={activeView} onNavigate={setActiveView} onChatToggle={toggleChat} />
          <div style={styles.content}>
            <div style={styles.mainArea}>
              <Suspense fallback={<div style={{ display: "flex", alignItems: "center", justifyContent: "center", height: "100%" }}><span className="system-text animate-glow">LOADING...</span></div>}>
                {renderView()}
              </Suspense>
            </div>
            <CommandBar onActivate={toggleChat} />
          </div>
        </div>
        {activeView !== "chat" && (
          <ChatPanel isOpen={chatOpen} onClose={closeChat} onNavigateToChat={() => { setChatOpen(false); setActiveView("chat"); }} />
        )}
        <AssistantHud
          visible={voiceRequestActive || aiState !== "idle" || assistantPhase !== "idle" || Boolean(assistantStatus) || Boolean(assistantThinking) || Boolean(assistantResponsePreview)}
          phase={assistantPhase}
          status={assistantStatus}
          thinking={assistantThinking}
          responsePreview={assistantResponsePreview}
        />
        <VoiceIndicator state={voiceState} onStop={stopVoice} />
        <ToastContainer />
        <ModelPullToast progress={modelPullProgress} visible={activeView !== "settings" && modelPullProgress !== null} />
      </div>
    </div>
    </ChatProvider>
  );
}

const styles: Record<string, React.CSSProperties> = {
  root: { height: "100vh", overflow: "hidden", position: "relative" as const },
  uiLayer: { position: "absolute" as const, top: 0, left: 0, right: 0, bottom: 0, zIndex: 2, display: "flex", flexDirection: "column" as const, overflow: "hidden" },
  titleBar: { height: 36, flexShrink: 0, display: "flex", justifyContent: "space-between", alignItems: "center", padding: "0 12px", background: "rgba(10, 14, 26, 0.4)", backdropFilter: "blur(12px)", borderBottom: "1px solid rgba(0, 180, 255, 0.08)" },
  titleText: { color: "rgba(0, 180, 255, 0.4)", fontFamily: "var(--font-mono)", fontSize: 10, letterSpacing: 3 },
  layout: { flex: 1, display: "flex", overflow: "hidden", minHeight: 0 },
  content: { flex: 1, display: "flex", flexDirection: "column", overflow: "hidden", minHeight: 0 },
  mainArea: { flex: 1, overflow: "auto", minHeight: 0 },
  lowerBtn: { background: "rgba(0, 180, 255, 0.1)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 4, padding: "2px 8px", color: "rgba(0, 180, 255, 0.7)", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 9, letterSpacing: 1 },
};
