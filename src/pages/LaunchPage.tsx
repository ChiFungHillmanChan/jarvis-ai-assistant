import { useEffect, useMemo, useState } from "react";
import { getLaunchReadiness, saveBetaTester, submitBetaFeedback } from "../lib/commands";
import type { LaunchReadiness } from "../lib/types";

export default function LaunchPage() {
  const [readiness, setReadiness] = useState<LaunchReadiness | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [segment, setSegment] = useState("Founder");
  const [useCase, setUseCase] = useState("");
  const [feedback, setFeedback] = useState("");
  const [rating, setRating] = useState(4);
  const [category, setCategory] = useState("activation");
  const [savedEmail, setSavedEmail] = useState("");
  const [notice, setNotice] = useState<string | null>(null);

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      setReadiness(await getLaunchReadiness());
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void refresh();
  }, []);

  const awsScore = useMemo(() => {
    if (!readiness) return 0;
    return [
      readiness.aws_credentials_present,
      Boolean(readiness.aws_region),
      Boolean(readiness.bedrock_model_id),
      readiness.provider_chain_has_bedrock,
    ].filter(Boolean).length;
  }, [readiness]);

  async function handleBetaSubmit(e: React.FormEvent) {
    e.preventDefault();
    setNotice(null);
    setError(null);
    try {
      const tester = await saveBetaTester({
        name,
        email,
        segment,
        use_case: useCase,
      });
      setSavedEmail(tester.email);
      setNotice("Beta tester saved. Use this email when submitting feedback.");
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleFeedbackSubmit(e: React.FormEvent) {
    e.preventDefault();
    setNotice(null);
    setError(null);
    try {
      await submitBetaFeedback({
        tester_email: savedEmail || email,
        category,
        rating,
        message: feedback,
      });
      setFeedback("");
      setNotice("Feedback saved for product validation.");
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  if (loading && !readiness) {
    return <div style={styles.center}><div className="system-text animate-glow">LOADING LAUNCH CONTROL...</div></div>;
  }

  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <div>
          <div className="system-text" style={styles.eyebrow}>AWS STARTUP LAUNCH</div>
          <h1 style={styles.title}>Beta Control Plane</h1>
        </div>
        <div style={styles.stage}>{readiness?.launch_stage?.replace("_", " ").toUpperCase() || "PRIVATE BETA"}</div>
      </div>

      {error && <div style={styles.error}>{error}</div>}
      {notice && <div style={styles.notice}>{notice}</div>}

      <div style={styles.grid}>
        <section className="panel" style={styles.panel}>
          <div className="label" style={styles.sectionTitle}>AWS READINESS</div>
          <div style={styles.scoreRow}>
            <div style={styles.score}>{awsScore}/4</div>
            <div style={styles.scoreText}>Bedrock launch checks complete</div>
          </div>
          <CheckRow label="AWS credentials" ok={Boolean(readiness?.aws_credentials_present)} detail="Required for Bedrock runtime calls" />
          <CheckRow label="Bedrock region" ok={Boolean(readiness?.aws_region)} detail={readiness?.aws_region || "Set AWS_BEDROCK_REGION"} />
          <CheckRow label="Bedrock model" ok={Boolean(readiness?.bedrock_model_id)} detail={readiness?.bedrock_model_id || "Set AWS_BEDROCK_MODEL_ID"} />
          <CheckRow label="Provider chain" ok={Boolean(readiness?.provider_chain_has_bedrock)} detail="Add Bedrock Claude in Settings" />
        </section>

        <section className="panel" style={styles.panel}>
          <div className="label" style={styles.sectionTitle}>COMMERCIAL SIGNAL</div>
          <div style={styles.priceGrid}>
            <PriceBox name="Pro" price={`HK$${readiness?.pro_price_hkd || "148"}/mo`} detail="Solo founders and operators" />
            <PriceBox name="Team" price={`HK$${readiness?.team_price_hkd || "298"}/seat`} detail="Audit logs and admin policy" />
          </div>
          <div style={styles.metricRow}>
            <Metric label="WAITLIST" value={readiness?.waitlist_count ?? 0} />
            <Metric label="FEEDBACK" value={readiness?.feedback_count ?? 0} />
          </div>
        </section>

        <section className="panel" style={styles.panel}>
          <div className="label" style={styles.sectionTitle}>BETA ACCESS</div>
          <form onSubmit={handleBetaSubmit} style={styles.form}>
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Name" style={styles.input} />
            <input value={email} onChange={(e) => setEmail(e.target.value)} placeholder="Email" style={styles.input} />
            <select value={segment} onChange={(e) => setSegment(e.target.value)} style={styles.input}>
              <option>Founder</option>
              <option>Operator</option>
              <option>Developer</option>
              <option>Team lead</option>
            </select>
            <textarea value={useCase} onChange={(e) => setUseCase(e.target.value)} placeholder="What should JARVIS save time on first?" rows={4} style={styles.textarea} />
            <button type="submit" style={styles.actionBtn}>SAVE BETA TESTER</button>
          </form>
        </section>

        <section className="panel" style={styles.panel}>
          <div className="label" style={styles.sectionTitle}>TEST FEEDBACK</div>
          <form onSubmit={handleFeedbackSubmit} style={styles.form}>
            <select value={category} onChange={(e) => setCategory(e.target.value)} style={styles.input}>
              <option value="activation">Activation</option>
              <option value="aws">AWS setup</option>
              <option value="workflow">Workflow reliability</option>
              <option value="pricing">Pricing</option>
            </select>
            <input type="number" min={1} max={5} value={rating} onChange={(e) => setRating(Number(e.target.value))} style={styles.input} />
            <textarea value={feedback} onChange={(e) => setFeedback(e.target.value)} placeholder="What broke, what saved time, or what would make this worth paying for?" rows={6} style={styles.textarea} />
            <button type="submit" style={styles.actionBtn}>SAVE FEEDBACK</button>
          </form>
        </section>
      </div>
    </div>
  );
}

function CheckRow({ label, ok, detail }: { label: string; ok: boolean; detail: string }) {
  return (
    <div style={styles.checkRow}>
      <div style={{ ...styles.statusDot, background: ok ? "rgba(16, 185, 129, 0.8)" : "rgba(255, 180, 0, 0.8)" }} />
      <div style={{ flex: 1 }}>
        <div style={styles.checkLabel}>{label}</div>
        <div style={styles.checkDetail}>{detail}</div>
      </div>
      <div style={ok ? styles.pass : styles.warn}>{ok ? "READY" : "TODO"}</div>
    </div>
  );
}

function PriceBox({ name, price, detail }: { name: string; price: string; detail: string }) {
  return (
    <div style={styles.priceBox}>
      <div style={styles.priceName}>{name}</div>
      <div style={styles.price}>{price}</div>
      <div style={styles.priceDetail}>{detail}</div>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: number }) {
  return (
    <div style={styles.metric}>
      <div style={styles.metricValue}>{value}</div>
      <div style={styles.metricLabel}>{label}</div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: { padding: 24, paddingBottom: 48 },
  center: { display: "flex", alignItems: "center", justifyContent: "center", height: "100%" },
  header: { display: "flex", justifyContent: "space-between", alignItems: "flex-start", marginBottom: 18 },
  eyebrow: { fontSize: 12, letterSpacing: 4, marginBottom: 8 },
  title: { margin: 0, color: "rgba(0, 180, 255, 0.9)", fontSize: 28, fontWeight: 500 },
  stage: { color: "rgba(0, 180, 255, 0.65)", border: "1px solid rgba(0, 180, 255, 0.25)", borderRadius: 6, padding: "7px 10px", fontFamily: "var(--font-mono)", fontSize: 10 },
  grid: { display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12, alignItems: "start" },
  panel: { padding: 16 },
  sectionTitle: { marginBottom: 12 },
  scoreRow: { display: "flex", alignItems: "center", gap: 14, marginBottom: 14 },
  score: { width: 52, height: 52, borderRadius: 8, border: "1px solid rgba(0, 180, 255, 0.35)", display: "flex", alignItems: "center", justifyContent: "center", color: "rgba(0, 180, 255, 0.9)", fontFamily: "var(--font-mono)", fontSize: 18 },
  scoreText: { color: "rgba(0, 180, 255, 0.58)", fontSize: 12 },
  checkRow: { display: "flex", alignItems: "center", gap: 10, padding: "9px 0", borderTop: "1px solid rgba(0, 180, 255, 0.08)" },
  statusDot: { width: 8, height: 8, borderRadius: "50%", flexShrink: 0 },
  checkLabel: { color: "rgba(0, 180, 255, 0.8)", fontSize: 12 },
  checkDetail: { color: "rgba(0, 180, 255, 0.42)", fontSize: 10, marginTop: 2, fontFamily: "var(--font-mono)" },
  pass: { color: "rgba(16, 185, 129, 0.8)", fontSize: 10, fontFamily: "var(--font-mono)" },
  warn: { color: "rgba(255, 180, 0, 0.85)", fontSize: 10, fontFamily: "var(--font-mono)" },
  priceGrid: { display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 },
  priceBox: { border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 8, padding: 12, background: "rgba(0, 180, 255, 0.035)" },
  priceName: { color: "rgba(0, 180, 255, 0.55)", fontSize: 11, fontFamily: "var(--font-mono)" },
  price: { color: "rgba(0, 180, 255, 0.9)", fontSize: 18, marginTop: 6 },
  priceDetail: { color: "rgba(0, 180, 255, 0.42)", fontSize: 11, marginTop: 4 },
  metricRow: { display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10, marginTop: 12 },
  metric: { borderTop: "1px solid rgba(0, 180, 255, 0.12)", paddingTop: 10 },
  metricValue: { color: "rgba(0, 180, 255, 0.85)", fontSize: 24, fontFamily: "var(--font-mono)" },
  metricLabel: { color: "rgba(0, 180, 255, 0.42)", fontSize: 10, fontFamily: "var(--font-mono)" },
  form: { display: "flex", flexDirection: "column", gap: 8 },
  input: { background: "rgba(0, 180, 255, 0.03)", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 6, padding: "8px 10px", color: "rgba(0, 180, 255, 0.8)", fontSize: 12, fontFamily: "var(--font-mono)", outline: "none" },
  textarea: { background: "rgba(0, 180, 255, 0.03)", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 6, padding: "8px 10px", color: "rgba(0, 180, 255, 0.8)", fontSize: 12, fontFamily: "var(--font-sans)", outline: "none", resize: "vertical" },
  actionBtn: { background: "rgba(0, 180, 255, 0.09)", border: "1px solid rgba(0, 180, 255, 0.32)", borderRadius: 6, padding: "9px 12px", color: "rgba(0, 180, 255, 0.82)", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 11 },
  error: { color: "rgba(255, 100, 100, 0.9)", border: "1px solid rgba(255, 100, 100, 0.25)", borderRadius: 6, padding: 10, marginBottom: 12, fontSize: 12 },
  notice: { color: "rgba(16, 185, 129, 0.88)", border: "1px solid rgba(16, 185, 129, 0.25)", borderRadius: 6, padding: 10, marginBottom: 12, fontSize: 12 },
};
