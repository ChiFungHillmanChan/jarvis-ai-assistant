import { useTauriCommand } from "../hooks/useTauriCommand";

export default function NotionCard() {
  const { data: count } = useTauriCommand<number>("get_notion_stats");
  return (
    <div className="panel" style={{ padding: 12 }}>
      <div className="label">NOTION</div>
      <div style={{ color: "rgba(0, 180, 255, 0.8)", fontSize: 20, fontWeight: 200, marginTop: 6 }}>{count ?? "--"}</div>
      <div style={{ color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginTop: 4 }}>
        {count != null ? `page${count !== 1 ? "s" : ""} synced` : "not connected"}
      </div>
    </div>
  );
}
