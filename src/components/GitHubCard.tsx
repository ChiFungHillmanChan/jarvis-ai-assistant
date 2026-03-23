import { useTauriCommand } from "../hooks/useTauriCommand";
import type { GitHubStats } from "../lib/types";

export default function GitHubCard() {
  const { data: stats } = useTauriCommand<GitHubStats>("get_github_stats");
  if (!stats) {
    return (
      <div className="panel" style={{ padding: 12 }}>
        <div className="label">GITHUB</div>
        <div style={{ color: "rgba(0, 180, 255, 0.8)", fontSize: 20, fontWeight: 200, marginTop: 6 }}>--</div>
        <div style={{ color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginTop: 4 }}>not connected</div>
      </div>
    );
  }
  const total = stats.open_prs + stats.assigned_issues + stats.review_requested;
  return (
    <div className="panel" style={{ padding: 12 }}>
      <div className="label">GITHUB</div>
      <div style={{ color: "rgba(0, 180, 255, 0.8)", fontSize: 20, fontWeight: 200, marginTop: 6 }}>{total}</div>
      <div style={{ color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginTop: 4 }}>
        {stats.open_prs} PRs / {stats.assigned_issues} issues
      </div>
      {stats.review_requested > 0 && (
        <div style={{ color: "rgba(255, 180, 0, 0.7)", fontSize: 9, marginTop: 4 }}>
          {stats.review_requested} review requested
        </div>
      )}
    </div>
  );
}
