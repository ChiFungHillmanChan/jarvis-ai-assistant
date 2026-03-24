import GreetingHeader from "../components/GreetingHeader";
import Timeline from "../components/Timeline";
import StatsPanel from "../components/StatsPanel";
import EmailRuleSuggestion from "../components/EmailRuleSuggestion";
import Briefing from "../components/Briefing";
import NotificationBanner from "../components/NotificationBanner";
import type { DashboardData } from "../lib/types";
import { useTauriCommand } from "../hooks/useTauriCommand";

export default function Dashboard() {
  const { data, error, loading } = useTauriCommand<DashboardData>("get_dashboard_data");

  if (error) return (
    <div style={styles.error}>
      <div className="system-text">SYSTEM ERROR</div>
      <div style={{ marginTop: 8, color: "var(--accent-urgent)" }}>{error}</div>
    </div>
  );
  if (loading || !data) return (
    <div style={styles.loading}><div className="system-text animate-glow">INITIALIZING...</div></div>
  );
  return (
    <div style={styles.container}>
      <div style={styles.main}>
        <Briefing />
        <GreetingHeader greeting={data.greeting} taskCount={data.task_count} />
        <NotificationBanner />
        <EmailRuleSuggestion />
        <Timeline tasks={data.pending_tasks} />
      </div>
      <div style={styles.stats}><StatsPanel taskCount={data.task_count} /></div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: { display: "flex", height: "100%", gap: 16, padding: 16 },
  main: { flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" },
  stats: { flexShrink: 0 },
  loading: { display: "flex", alignItems: "center", justifyContent: "center", height: "100%" },
  error: { display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", height: "100%" },
};
