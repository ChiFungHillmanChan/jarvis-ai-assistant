import { useState, useEffect } from "react";
import LocalMailSetup from "./LocalMailSetup";
import { useUiLocale } from "../i18n";
import "../styles/setup.css";
import { listen } from "@tauri-apps/api/event";
import {
  checkSystemCompatibility,
  checkBackendPrerequisites,
  getRecommendedModels,
  pullModel,
  cancelModelPull,
  addLocalEndpoint,
  listEndpointModels,
} from "../lib/commands";
import type {
  SystemCompatibility,
  PrerequisiteCheck,
  RecommendedModel,
  ModelPullProgress,
  LocalEndpoint,
  LocalModel,
} from "../lib/types";

interface LocalAiSetupProps {
  endpoints: LocalEndpoint[];
  onEndpointAdded: (ep: LocalEndpoint) => void;
  onModelsLoaded: (endpointId: string, models: LocalModel[]) => void;
  onAddToChain: (providerType: string, endpointId?: string, modelId?: string) => void;
}

export type BackendTab = "ollama" | "vllm" | "generic";

/**
 * The engine setup this panel shows: this Mac's hardware, the chosen
 * backend's prerequisites, Ollama's recommended models and their download,
 * and connecting a vLLM or generic server. Shared with Settings > Advanced >
 * Local AI, which lays the same steps out as rows.
 */
export function useLocalEngines({ endpoints, onEndpointAdded, onModelsLoaded }: Omit<LocalAiSetupProps, "onAddToChain">) {
  const { text } = useUiLocale();
  const [system, setSystem] = useState<SystemCompatibility | null>(null);
  const [tab, setTab] = useState<BackendTab>("ollama");
  const [checks, setChecks] = useState<PrerequisiteCheck[]>([]);
  const [models, setModels] = useState<RecommendedModel[]>([]);
  const [pulling, setPulling] = useState<ModelPullProgress | null>(null);
  const [checkingPrereqs, setCheckingPrereqs] = useState(false);
  const [vllmUrl, setVllmUrl] = useState("http://localhost:8000");
  const [genericUrl, setGenericUrl] = useState("http://localhost:8080");
  const [connectBusy, setConnectBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Load system info once
  useEffect(() => {
    checkSystemCompatibility().then(setSystem).catch(() => {});
  }, []);

  // Load prerequisites when tab changes
  useEffect(() => {
    setCheckingPrereqs(true);
    setChecks([]);
    const url = tab === "vllm" ? vllmUrl : tab === "generic" ? genericUrl : undefined;
    checkBackendPrerequisites(tab, url)
      .then(setChecks)
      .catch(() => {})
      .finally(() => setCheckingPrereqs(false));
  }, [tab]);

  // Load recommended models for Ollama tab
  useEffect(() => {
    if (tab !== "ollama") return;
    const ollamaEndpoint = endpoints.find((e) => e.backend_type === "ollama");
    getRecommendedModels(ollamaEndpoint?.id).then(setModels).catch(() => {});
  }, [tab, endpoints]);

  // Listen for pull progress
  useEffect(() => {
    const unlisten = listen<ModelPullProgress>("model-pull-progress", (event) => {
      const p = event.payload;
      if (p.status === "complete" || p.status === "error") {
        setPulling(null);
        if (p.status === "complete") {
          const ollamaEndpoint = endpoints.find((e) => e.backend_type === "ollama");
          getRecommendedModels(ollamaEndpoint?.id).then(setModels).catch(() => {});
          checkBackendPrerequisites("ollama").then(setChecks).catch(() => {});
        }
        if (p.error) setError(p.error);
      } else {
        setPulling(p);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [endpoints]);

  async function handlePull(modelId: string) {
    setError(null);
    let ollamaEndpoint = endpoints.find((e) => e.backend_type === "ollama");
    if (!ollamaEndpoint) {
      try {
        const ep = await addLocalEndpoint("Ollama", "http://localhost:11434", "ollama");
        onEndpointAdded(ep);
        ollamaEndpoint = ep;
      } catch (e) {
        setError(String(e));
        return;
      }
    }
    try {
      await pullModel(ollamaEndpoint.id, modelId);
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleConnect(backendType: BackendTab) {
    const url = backendType === "vllm" ? vllmUrl : genericUrl;
    setConnectBusy(true);
    setError(null);
    try {
      const ep = await addLocalEndpoint(
        backendType === "vllm" ? "vLLM" : "Generic",
        url,
        backendType
      );
      onEndpointAdded(ep);
      const epModels = await listEndpointModels(ep.id);
      onModelsLoaded(ep.id, epModels);
      checkBackendPrerequisites(backendType, url).then(setChecks).catch(() => {});
    } catch (e) {
      setError(String(e));
    } finally {
      setConnectBusy(false);
    }
  }

  async function handleRefreshPrereqs() {
    setCheckingPrereqs(true);
    const url = tab === "vllm" ? vllmUrl : tab === "generic" ? genericUrl : undefined;
    checkBackendPrerequisites(tab, url)
      .then(setChecks)
      .catch(() => {})
      .finally(() => setCheckingPrereqs(false));
  }

  const gpuLabel = system?.gpu.has_metal
    ? "Apple Silicon (Metal)"
    : system?.gpu.has_cuda
    ? `NVIDIA (CUDA ${system.gpu.cuda_version || ""})`
    : text("No GPU acceleration", "未提供 GPU 加速");

  return {
    system, gpuLabel, tab, setTab, checks, checkingPrereqs, refreshPrereqs: handleRefreshPrereqs, models, pulling, pull: handlePull,
    vllmUrl, setVllmUrl, genericUrl, setGenericUrl, connect: handleConnect, connectBusy, error,
  };
}

export default function LocalAiSetup({ endpoints, onEndpointAdded, onModelsLoaded, onAddToChain }: LocalAiSetupProps) {
  const { text } = useUiLocale();
  const [mailSetupBusy, setMailSetupBusy] = useState(false);
  const {
    system, gpuLabel, tab, setTab, checks, checkingPrereqs, refreshPrereqs: handleRefreshPrereqs, models, pulling, pull: handlePull,
    vllmUrl, setVllmUrl, genericUrl, setGenericUrl, connect: handleConnect, connectBusy, error,
  } = useLocalEngines({ endpoints, onEndpointAdded, onModelsLoaded });

  return (
    <div className="panel" style={styles.panel}>
      <LocalMailSetup onBusyChange={setMailSetupBusy} />
      <details><summary>{text("Advanced engines for Assistant", "AI 助理進階引擎設定")}</summary>
      <fieldset disabled={mailSetupBusy} style={{ border: 0, padding: 0, margin: 0, minWidth: 0 }}>
      <div className="label" style={styles.sectionTitle}>{text("LOCAL AI SETUP", "本機 AI 設定")}</div>

      {system && (
        <div style={styles.systemBar}>
          <span style={styles.systemStat}>{system.total_ram_gb.toFixed(0)} GB RAM</span>
          <span style={styles.systemDivider}>|</span>
          <span style={styles.systemStat}>{gpuLabel}</span>
          <span style={styles.systemDivider}>|</span>
          <span style={styles.systemStat}>{text("Suggested upper model size", "建議模型大小上限")} {system.recommended_max_params}</span>
        </div>
      )}

      <div style={styles.tabRow}>
        {(["ollama", "vllm", "generic"] as const).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            style={{ ...styles.tab, ...(tab === t ? styles.tabActive : {}) }}
          >
            {t.toUpperCase()}
          </button>
        ))}
        <button onClick={handleRefreshPrereqs} disabled={checkingPrereqs} style={styles.refreshBtn}>
          {checkingPrereqs ? "..." : text("REFRESH", "重新檢查")}
        </button>
      </div>

      <div style={styles.checklistSection}>
        {checks.map((check, i) => (
          <div key={i} style={styles.checkRow}>
            <span style={{ ...styles.checkDot, background: check.passed ? "rgba(16, 185, 129, 0.7)" : "rgba(255, 100, 100, 0.7)" }} />
            <div style={{ flex: 1 }}>
              <div style={{ color: "rgba(0, 180, 255, 0.8)", fontSize: 11 }}>{check.name}</div>
              {!check.passed && check.fix_command && (
                <div style={styles.fixRow}>
                  <code style={styles.fixCode}>{check.fix_command}</code>
                  <button onClick={() => navigator.clipboard.writeText(check.fix_command!)} style={styles.copyBtn}>{text("COPY", "複製")}</button>
                </div>
              )}
              {!check.passed && !check.fix_command && check.fix_label && (
                <div style={styles.fixHint}>{check.fix_label}</div>
              )}
            </div>
          </div>
        ))}
        {checks.length === 0 && !checkingPrereqs && (
          <div style={styles.emptyText}>{text("Click REFRESH to check prerequisites", "按「重新檢查」核對所需條件")}</div>
        )}
      </div>

      {tab === "ollama" && models.length > 0 && (
        <div style={styles.modelsSection}>
          <div style={styles.modelsLabel}>{text("RECOMMENDED MODELS", "模型建議")}</div>
          {models.map((m) => (
            <div key={m.id} style={styles.modelCard}>
              <div style={styles.modelHeader}>
                <span style={styles.modelName}>{m.name}</span>
                <span style={styles.modelSize}>{m.download_size_gb} GB</span>
                {m.tool_capable && <span style={styles.toolBadge}>{text("tools", "工具")}</span>}
              </div>
              <div style={styles.modelDesc}>{m.description}</div>
              <div style={styles.modelActions}>
                {m.already_pulled ? (
                  <>
                    <span style={styles.readyBadge}>{text("READY", "已下載")}</span>
                    <button onClick={() => {
                      const ep = endpoints.find((e) => e.backend_type === "ollama");
                      if (ep) onAddToChain("local", ep.id, m.id);
                    }} style={styles.chainBtn}>{text("+ CHAIN", "加入助理模型鏈")}</button>
                  </>
                ) : (
                  <button
                    onClick={() => handlePull(m.id)}
                    disabled={pulling !== null}
                    style={styles.pullBtn}
                  >
                    {pulling?.model === m.id ? `${pulling.percent}%` : text("PULL", "下載")}
                  </button>
                )}
              </div>
            </div>
          ))}
        </div>
      )}

      {pulling && (
        <div style={styles.pullProgress}>
          <div style={styles.pullLabel}>{text("Downloading", "下載中")} {pulling.model}... {pulling.percent}%</div>
          <div style={styles.progressShell}>
            <div style={{ ...styles.progressFill, width: `${pulling.percent}%` }} />
          </div>
          <button onClick={() => cancelModelPull(pulling?.jobId)} style={styles.cancelBtn}>{text("CANCEL", "取消")}</button>
        </div>
      )}

      {(tab === "vllm" || tab === "generic") && (
        <div style={styles.urlSection}>
          <div style={{ display: "flex", gap: 6 }}>
            <input
              value={tab === "vllm" ? vllmUrl : genericUrl}
              onChange={(e) => tab === "vllm" ? setVllmUrl(e.target.value) : setGenericUrl(e.target.value)}
              placeholder={tab === "vllm" ? "http://localhost:8000" : "http://localhost:8080"}
              style={styles.urlInput}
            />
            <button onClick={() => handleConnect(tab)} disabled={connectBusy} style={styles.connectBtn}>
              {connectBusy ? "..." : text("CONNECT", "連接")}
            </button>
          </div>
        </div>
      )}

      {error && <div style={styles.errorText}>{error}</div>}
      </fieldset></details>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  panel: { padding: 16 },
  sectionTitle: { marginBottom: 12 },
  systemBar: { display: "flex", alignItems: "center", gap: 8, marginBottom: 12, padding: "6px 10px", background: "rgba(0, 180, 255, 0.04)", borderRadius: 6, border: "1px solid rgba(0, 180, 255, 0.1)" },
  systemStat: { color: "rgba(0, 180, 255, 0.6)", fontSize: 10, fontFamily: "var(--font-mono)" },
  systemDivider: { color: "rgba(0, 180, 255, 0.2)", fontSize: 10 },
  tabRow: { display: "flex", gap: 4, marginBottom: 12, alignItems: "center" },
  tab: { background: "rgba(0, 180, 255, 0.04)", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 4, padding: "4px 12px", color: "rgba(0, 180, 255, 0.5)", fontSize: 9, fontFamily: "var(--font-mono)", letterSpacing: 1, cursor: "pointer" },
  tabActive: { background: "rgba(0, 180, 255, 0.12)", borderColor: "rgba(0, 180, 255, 0.4)", color: "rgba(0, 180, 255, 0.9)" },
  refreshBtn: { marginLeft: "auto", background: "transparent", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 4, padding: "4px 8px", color: "rgba(0, 180, 255, 0.4)", fontSize: 8, fontFamily: "var(--font-mono)", cursor: "pointer" },
  checklistSection: { marginBottom: 12 },
  checkRow: { display: "flex", alignItems: "flex-start", gap: 8, marginBottom: 8 },
  checkDot: { width: 8, height: 8, borderRadius: "50%", marginTop: 3, flexShrink: 0 },
  fixRow: { display: "flex", alignItems: "center", gap: 6, marginTop: 4 },
  fixCode: { color: "rgba(0, 180, 255, 0.5)", fontFamily: "var(--font-mono)", fontSize: 9, background: "rgba(0, 180, 255, 0.04)", padding: "2px 6px", borderRadius: 3 },
  copyBtn: { background: "transparent", border: "1px solid rgba(0, 180, 255, 0.2)", borderRadius: 3, padding: "1px 6px", color: "rgba(0, 180, 255, 0.4)", fontSize: 8, fontFamily: "var(--font-mono)", cursor: "pointer" },
  fixHint: { color: "rgba(0, 180, 255, 0.35)", fontSize: 9, marginTop: 2 },
  emptyText: { color: "rgba(0, 180, 255, 0.3)", fontSize: 10 },
  modelsSection: { marginBottom: 12 },
  modelsLabel: { color: "rgba(0, 180, 255, 0.4)", fontSize: 9, fontFamily: "var(--font-mono)", letterSpacing: 1, marginBottom: 8 },
  modelCard: { background: "rgba(0, 180, 255, 0.03)", border: "1px solid rgba(0, 180, 255, 0.1)", borderRadius: 6, padding: 10, marginBottom: 6 },
  modelHeader: { display: "flex", alignItems: "center", gap: 8 },
  modelName: { color: "rgba(0, 180, 255, 0.8)", fontSize: 11, fontWeight: 600, flex: 1 },
  modelSize: { color: "rgba(0, 180, 255, 0.4)", fontSize: 9, fontFamily: "var(--font-mono)" },
  toolBadge: { color: "rgba(16, 185, 129, 0.7)", fontSize: 8, fontFamily: "var(--font-mono)", border: "1px solid rgba(16, 185, 129, 0.2)", borderRadius: 3, padding: "1px 4px" },
  modelDesc: { color: "rgba(0, 180, 255, 0.4)", fontSize: 9, marginTop: 4 },
  modelActions: { display: "flex", alignItems: "center", gap: 6, marginTop: 6 },
  readyBadge: { color: "rgba(16, 185, 129, 0.7)", fontSize: 9, fontFamily: "var(--font-mono)", letterSpacing: 1 },
  chainBtn: { background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 4, padding: "2px 8px", color: "rgba(0, 180, 255, 0.8)", fontSize: 9, fontFamily: "var(--font-mono)", cursor: "pointer" },
  pullBtn: { background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 4, padding: "3px 10px", color: "rgba(0, 180, 255, 0.8)", fontSize: 9, fontFamily: "var(--font-mono)", cursor: "pointer" },
  pullProgress: { marginBottom: 12 },
  pullLabel: { color: "rgba(0, 180, 255, 0.6)", fontSize: 10, fontFamily: "var(--font-mono)", marginBottom: 4 },
  progressShell: { width: "100%", height: 6, borderRadius: 999, overflow: "hidden", background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.15)" },
  progressFill: { height: "100%", background: "linear-gradient(90deg, rgba(0, 180, 255, 0.65), rgba(96, 165, 250, 0.95))", transition: "width 200ms ease-out" },
  cancelBtn: { background: "transparent", border: "1px solid rgba(255, 100, 100, 0.3)", borderRadius: 4, padding: "2px 8px", color: "rgba(255, 100, 100, 0.6)", fontSize: 8, fontFamily: "var(--font-mono)", cursor: "pointer", marginTop: 4 },
  urlSection: { marginTop: 8 },
  urlInput: { flex: 1, background: "rgba(0, 180, 255, 0.03)", border: "1px solid rgba(0, 180, 255, 0.15)", borderRadius: 6, padding: "6px 10px", color: "rgba(0, 180, 255, 0.8)", fontSize: 11, fontFamily: "var(--font-mono)", outline: "none" },
  connectBtn: { background: "rgba(0, 180, 255, 0.08)", border: "1px solid rgba(0, 180, 255, 0.3)", borderRadius: 6, padding: "6px 12px", color: "rgba(0, 180, 255, 0.8)", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 10 },
  errorText: { color: "rgba(255, 100, 100, 0.85)", fontSize: 11, marginTop: 8 },
};
