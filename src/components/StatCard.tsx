interface StatCardProps { label: string; value: number | string; detail?: string; }
export default function StatCard({ label, value, detail }: StatCardProps) {
  return (
    <div className="panel" style={styles.card}>
      <div className="label">{label}</div>
      <div style={styles.value}>{value}</div>
      {detail && <div style={styles.detail}>{detail}</div>}
    </div>
  );
}
const styles: Record<string, React.CSSProperties> = {
  card: { padding: 12 },
  value: { color: "rgba(0, 180, 255, 0.8)", fontSize: 20, fontWeight: 200, marginTop: 6 },
  detail: { color: "rgba(0, 180, 255, 0.4)", fontSize: 10, marginTop: 4 },
};
