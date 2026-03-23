import { useTauriCommand } from "../hooks/useTauriCommand";
import type { CronJobView } from "../lib/types";

export default function CronCard() {
  const { data: jobs } = useTauriCommand<CronJobView[]>("get_cron_jobs");
  const active = jobs?.filter((j) => j.status === "active").length ?? 0;
  const lastRun = jobs?.find((j) => j.last_run)?.last_run;
  return (
    <div className="panel" style={styles.card}>
      <div className="label">CRON JOBS</div>
      <div style={styles.value}>{active}</div>
      <div style={styles.detail}>{active === 0 ? "none active" : "active"}</div>
      {lastRun && <div style={styles.last}>Last run: {new Date(lastRun).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</div>}
    </div>
  );
}
const styles: Record<string, React.CSSProperties> = {
  card: { padding: 12 },
  value: { color: "rgba(0, 180, 255, 0.8)", fontSize: 20, fontWeight: 200, marginTop: 6 },
  detail: { color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginTop: 4 },
  last: { color: "rgba(16, 185, 129, 0.6)", fontSize: 9, marginTop: 6 },
};
