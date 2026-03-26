import { memo } from "react";
import StatCard from "./StatCard";
import CalendarCard from "./CalendarCard";
import CronCard from "./CronCard";
import GitHubCard from "./GitHubCard";
import NotionCard from "./NotionCard";
import { useTauriCommand } from "../hooks/useTauriCommand";
import type { EmailStats } from "../lib/types";

interface StatsPanelProps { taskCount: number; }

export default memo(function StatsPanel({ taskCount }: StatsPanelProps) {
  const { data: emailStats } = useTauriCommand<EmailStats>("get_email_stats");
  return (
    <div style={styles.container}>
      <StatCard label="TASKS" value={taskCount} detail="pending" />
      <StatCard label="EMAIL" value={emailStats?.unread ?? "--"} detail={emailStats ? `${emailStats.unread} unread` : "not connected"} />
      <CalendarCard />
      <GitHubCard />
      <NotionCard />
      <CronCard />
    </div>
  );
})
const styles: Record<string, React.CSSProperties> = {
  container: { width: 160, display: "flex", flexDirection: "column", gap: 10, overflowY: "auto" },
};
