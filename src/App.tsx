import { useState, useCallback } from "react";
import Sidebar from "./components/Sidebar";
import ChatPanel from "./components/ChatPanel";
import CommandBar from "./components/CommandBar";
import Dashboard from "./pages/Dashboard";
import Settings from "./pages/Settings";
import CronDashboard from "./pages/CronDashboard";
import { useKeyboard } from "./hooks/useKeyboard";
import VoiceIndicator from "./components/VoiceIndicator";
import { useVoiceState } from "./hooks/useVoiceState";

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
      case "settings": return <Settings />;
      case "cron": return <CronDashboard />;
      case "home": default: return <Dashboard />;
    }
  }

  return (
    <div style={styles.root}>
      <div className="drag-region" style={styles.dragBar} />
      <div style={styles.layout}>
        <Sidebar activeView={activeView} onNavigate={setActiveView} onChatToggle={toggleChat} />
        <div style={styles.content}>
          <div style={styles.mainArea}>{renderView()}</div>
          <CommandBar onActivate={toggleChat} />
        </div>
      </div>
      <ChatPanel isOpen={chatOpen} isFullScreen={chatFullScreen} onClose={closeChat} onToggleFullScreen={() => setChatFullScreen((prev) => !prev)} />
      <VoiceIndicator state={voiceState} onStop={stopVoice} />
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  root: { height: "100vh", display: "flex", flexDirection: "column", overflow: "hidden" },
  dragBar: { height: 28, flexShrink: 0 },
  layout: { flex: 1, display: "flex", overflow: "hidden" },
  content: { flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" },
  mainArea: { flex: 1, overflow: "hidden" },
};
