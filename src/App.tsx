import { useState, useCallback } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
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

  const toggleChat = useCallback(() => { setChatOpen((prev) => !prev); setChatFullScreen(false); }, []);
  const closeChat = useCallback(() => { setChatOpen(false); setChatFullScreen(false); }, []);

  const { state: voiceState, startVoice, stopVoice } = useVoiceState();

  useKeyboard({
    "cmd+k": toggleChat,
    escape: closeChat,
    "cmd+shift+j": () => {
      if (voiceState === "Listening") { stopVoice(); }
      else if (voiceState === "Idle") { startVoice(); }
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
      {/* 3D atom field background -- renders behind UI */}
      <JarvisScene />

      {/* UI layer -- floats above 3D */}
      <div style={styles.uiLayer}>
        <div className="drag-region" style={styles.titleBar} onMouseDown={(e) => { if ((e.target as HTMLElement).closest('.no-drag')) return; getCurrentWindow().startDragging(); }}>
          <span style={styles.titleText}>JARVIS</span>
          <WindowControls />
        </div>
        <div style={styles.layout}>
        <Sidebar activeView={activeView} onNavigate={setActiveView} onChatToggle={toggleChat} />
        <div style={styles.content}>
          <div style={styles.mainArea}>{renderView()}</div>
          <CommandBar onActivate={toggleChat} />
        </div>
      </div>
        <ChatPanel isOpen={chatOpen} isFullScreen={chatFullScreen} onClose={closeChat} onToggleFullScreen={() => setChatFullScreen((prev) => !prev)} />
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
};
