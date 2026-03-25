import { useState, useCallback, useEffect, useRef, useMemo } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import Sidebar from "./components/Sidebar";
import ChatPanel from "./components/ChatPanel";
import CommandBar from "./components/CommandBar";
import Dashboard from "./pages/Dashboard";
import Settings from "./pages/Settings";
import CronDashboard from "./pages/CronDashboard";
import EmailPage from "./pages/EmailPage";
import CalendarPage from "./pages/CalendarPage";
import GitHubPage from "./pages/GitHubPage";
import NotionPage from "./pages/NotionPage";
import { useKeyboard } from "./hooks/useKeyboard";
import VoiceIndicator from "./components/VoiceIndicator";
import { useVoiceState } from "./hooks/useVoiceState";
import ToastContainer from "./components/Toast";
import WindowControls from "./components/WindowControls";
import JarvisScene from "./components/3d/JarvisScene";

export default function App() {
  const [activeView, setActiveView] = useState("home");
  const [chatOpen, setChatOpen] = useState(false);
  const [chatFullScreen, setChatFullScreen] = useState(false);
  const [wallpaperActive, setWallpaperActive] = useState(false);
  const [wallpaperRaised, setWallpaperRaised] = useState(false);
  const [aiState, setAiState] = useState<"idle" | "thinking" | "speaking">("idle");
  const [pageTransition, setPageTransition] = useState(false);
  const ttsAmplitudeRef = useRef(0);
  const [pendingToolCall, setPendingToolCall] = useState<string | null>(null);
  const prevView = useRef(activeView);

  const toggleChat = useCallback(() => { setChatOpen((prev) => !prev); setChatFullScreen(false); }, []);
  const closeChat = useCallback(() => { setChatOpen(false); setChatFullScreen(false); }, []);
  const handleToolCallConsumed = useCallback(() => setPendingToolCall(null), []);
  const toggleFullScreen = useCallback(() => setChatFullScreen((prev) => !prev), []);

  const { state: voiceState, startVoice, stopVoice } = useVoiceState();

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
    const unlistenAi = listen<{ state: "idle" | "thinking" | "speaking" }>("chat-state", (event) => {
      setAiState(event.payload.state);
      // Auto-open chat panel when AI starts thinking (e.g. from voice input)
      if (event.payload.state === "thinking") {
        setChatOpen(true);
      }
    });
    return () => { unlistenAi.then((fn) => fn()); };
  }, []);

  // Listen for TTS amplitude and tool call events
  useEffect(() => {
    const unlistenAmp = listen<{ amplitude: number }>("tts-amplitude", (event) => {
      // Write to ref instead of state -- JarvisScene reads via ref, no re-render cascade
      ttsAmplitudeRef.current = event.payload.amplitude;
    });
    const unlistenTool = listen<{ tool_name: string }>("chat-tool-call", (event) => {
      setPendingToolCall(event.payload.tool_name);
    });
    return () => {
      unlistenAmp.then((fn) => fn());
      unlistenTool.then((fn) => fn());
    };
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

  async function handleLowerToBackground() {
    await invoke("lower_wallpaper");
    setWallpaperRaised(false);
  }

  useKeyboard({
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
  });

  function renderView() {
    switch (activeView) {
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
    <div style={styles.root}>
      <JarvisScene activityLevel={activityLevel} ttsAmplitudeRef={ttsAmplitudeRef} pendingToolCall={pendingToolCall} onToolCallConsumed={handleToolCallConsumed} />

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
            <div style={styles.mainArea}>{renderView()}</div>
            <CommandBar onActivate={toggleChat} />
          </div>
        </div>
        <ChatPanel isOpen={chatOpen} isFullScreen={chatFullScreen} onClose={closeChat} onToggleFullScreen={toggleFullScreen} />
        <VoiceIndicator state={voiceState} onStop={stopVoice} />
        <ToastContainer />
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  root: { height: "100vh", overflow: "hidden", position: "relative" as const },
  uiLayer: { position: "absolute" as const, top: 0, left: 0, right: 0, bottom: 0, zIndex: 2, display: "flex", flexDirection: "column" as const, overflow: "hidden" },
  titleBar: { height: 36, flexShrink: 0, display: "flex", justifyContent: "space-between", alignItems: "center", padding: "0 12px", background: "rgba(10, 14, 26, 0.4)", backdropFilter: "blur(12px)", borderBottom: "1px solid rgba(0, 180, 255, 0.08)" },
  titleText: { color: "rgba(0, 180, 255, 0.4)", fontFamily: "var(--font-mono)", fontSize: 10, letterSpacing: 3 },
  layout: { flex: 1, display: "flex", overflow: "hidden" },
  content: { flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" },
  mainArea: { flex: 1, overflow: "auto" },
  lowerBtn: { background: "rgba(0, 180, 255, 0.1)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 4, padding: "2px 8px", color: "rgba(0, 180, 255, 0.7)", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 9, letterSpacing: 1 },
};
