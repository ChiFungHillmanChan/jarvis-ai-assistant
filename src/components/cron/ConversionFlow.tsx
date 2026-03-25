import { useState, useRef, useCallback } from "react";
import type { CronJobView } from "../../lib/types";
import { createCustomCron } from "../../lib/commands";

type Phase = "idle" | "glowing" | "parsing" | "result" | "done" | "error";

interface ConversionFlowProps {
  onJobCreated: (job: CronJobView) => void;
}

export default function ConversionFlow({ onJobCreated }: ConversionFlowProps) {
  const [input, setInput] = useState("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [createdJob, setCreatedJob] = useState<CronJobView | null>(null);
  const [errorMsg, setErrorMsg] = useState("");
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const cleanup = useCallback(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const handleSubmit = useCallback(async () => {
    const text = input.trim();
    if (!text || phase !== "idle") return;
    cleanup();

    // Phase 1: glowing
    setPhase("glowing");

    await new Promise((r) => {
      timerRef.current = setTimeout(r, 300);
    });

    // Phase 2: parsing
    setPhase("parsing");

    try {
      const job = await createCustomCron(text);
      setCreatedJob(job);

      // Phase 3: result
      setPhase("result");

      await new Promise((r) => {
        timerRef.current = setTimeout(r, 500);
      });

      // Phase 4: done
      setPhase("done");
      onJobCreated(job);
      setInput("");

      // Reset after 2s
      timerRef.current = setTimeout(() => {
        setPhase("idle");
        setCreatedJob(null);
      }, 2000);
    } catch (e) {
      setErrorMsg(e instanceof Error ? e.message : String(e));
      setPhase("error");
      timerRef.current = setTimeout(() => {
        setPhase("idle");
        setErrorMsg("");
      }, 3000);
    }
  }, [input, phase, cleanup, onJobCreated]);

  const isProcessing = phase !== "idle";
  const showFlow = phase !== "idle";

  return (
    <div style={styles.wrapper}>
      {/* Input section */}
      <div style={styles.inputSection}>
        <div style={styles.inputLabel} className="system-text">
          NATURAL LANGUAGE INPUT
        </div>
        <div style={styles.inputRow}>
          <input
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleSubmit()}
            placeholder="e.g. Every Monday at 9am check email for spam..."
            disabled={isProcessing}
            style={{
              ...styles.input,
              filter: phase === "glowing" ? "drop-shadow(0 0 8px rgba(0, 180, 255, 0.6))" : "none",
            }}
          />
          <button
            onClick={handleSubmit}
            disabled={isProcessing || !input.trim()}
            style={{
              ...styles.submitBtn,
              cursor: isProcessing || !input.trim() ? "not-allowed" : "pointer",
              opacity: isProcessing || !input.trim() ? 0.5 : 1,
            }}
          >
            {isProcessing ? "PROCESSING..." : "+ NEW JOB"}
          </button>
        </div>
      </div>

      {/* Flow section */}
      {showFlow && (
        <div style={styles.flowSection}>
          {/* Arrow */}
          <div style={styles.arrow}>
            <svg width="16" height="24" viewBox="0 0 16 24">
              <path
                d="M8 0 L8 18 M3 14 L8 20 L13 14"
                stroke="rgba(0, 180, 255, 0.3)"
                strokeWidth="1.5"
                fill="none"
              />
            </svg>
          </div>

          {/* Parsing phase */}
          {phase === "parsing" && (
            <div style={styles.parsingCard} className="animate-glow">
              <span style={styles.parsingLabel}>AI PARSING</span>
              <span style={styles.parsingDots}>...</span>
            </div>
          )}

          {/* Result phase */}
          {phase === "result" && createdJob && (
            <div style={styles.resultRow}>
              <div style={styles.resultCard}>
                <div style={styles.resultLabel}>CRON EXPRESSION</div>
                <div style={styles.cronValue}>{createdJob.schedule}</div>
              </div>
              <div style={styles.resultCard}>
                <div style={styles.resultLabel}>SCHEDULE</div>
                <div style={styles.scheduleValue}>
                  {createdJob.description || createdJob.name}
                </div>
              </div>
            </div>
          )}

          {/* Done phase */}
          {phase === "done" && createdJob && (
            <div style={styles.doneCard}>
              <svg
                width="16"
                height="16"
                viewBox="0 0 16 16"
                style={{ flexShrink: 0 }}
              >
                <circle
                  cx="8"
                  cy="8"
                  r="7"
                  fill="none"
                  stroke="rgba(16, 185, 129, 0.6)"
                  strokeWidth="1.5"
                />
                <path
                  d="M4.5 8 L7 10.5 L11.5 5.5"
                  fill="none"
                  stroke="rgba(16, 185, 129, 0.8)"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              </svg>
              <span style={styles.doneName}>{createdJob.name}</span>
              <span style={styles.activeBadge}>ACTIVE</span>
            </div>
          )}

          {/* Error phase */}
          {phase === "error" && (
            <div style={styles.errorCard}>
              <span style={styles.errorLabel}>ERROR</span>
              <span style={styles.errorMsg}>{errorMsg}</span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  wrapper: {
    padding: "12px 0",
  },
  inputSection: {},
  inputLabel: {
    fontSize: 9,
    marginBottom: 6,
    letterSpacing: 1,
  },
  inputRow: {
    display: "flex",
    gap: 8,
  },
  input: {
    flex: 1,
    background: "rgba(0, 180, 255, 0.03)",
    border: "1px solid rgba(0, 180, 255, 0.15)",
    borderRadius: 6,
    padding: "8px 12px",
    color: "rgba(0, 180, 255, 0.8)",
    fontSize: 12,
    fontFamily: "var(--font-mono)",
    outline: "none",
    transition: "filter 0.3s, border-color 0.2s",
  },
  submitBtn: {
    background: "rgba(0, 180, 255, 0.08)",
    border: "1px solid rgba(0, 180, 255, 0.3)",
    borderRadius: 6,
    padding: "8px 16px",
    color: "rgba(0, 180, 255, 0.8)",
    fontFamily: "var(--font-mono)",
    fontSize: 10,
    letterSpacing: 0.5,
    whiteSpace: "nowrap",
    transition: "opacity 0.2s",
  },
  flowSection: {
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    gap: 8,
    paddingTop: 4,
  },
  arrow: {
    display: "flex",
    justifyContent: "center",
  },
  parsingCard: {
    display: "flex",
    alignItems: "center",
    gap: 8,
    padding: "8px 16px",
    border: "1px solid rgba(0, 180, 255, 0.2)",
    borderRadius: 6,
    background: "rgba(0, 180, 255, 0.04)",
  },
  parsingLabel: {
    color: "rgba(0, 180, 255, 0.7)",
    fontSize: 10,
    fontFamily: "var(--font-mono)",
    letterSpacing: 1,
  },
  parsingDots: {
    color: "rgba(0, 180, 255, 0.4)",
    fontSize: 12,
  },
  resultRow: {
    display: "flex",
    gap: 12,
    width: "100%",
    justifyContent: "center",
  },
  resultCard: {
    padding: "10px 16px",
    border: "1px solid rgba(0, 180, 255, 0.15)",
    borderRadius: 6,
    background: "rgba(0, 180, 255, 0.03)",
    minWidth: 140,
    textAlign: "center" as const,
  },
  resultLabel: {
    color: "rgba(0, 180, 255, 0.4)",
    fontSize: 8,
    fontFamily: "var(--font-mono)",
    letterSpacing: 1,
    marginBottom: 6,
  },
  cronValue: {
    color: "rgba(0, 180, 255, 0.9)",
    fontSize: 16,
    fontFamily: "var(--font-mono)",
    fontWeight: 600,
    textShadow: "0 0 8px rgba(0, 180, 255, 0.3)",
  },
  scheduleValue: {
    color: "rgba(0, 180, 255, 0.7)",
    fontSize: 12,
    lineHeight: "1.4",
  },
  doneCard: {
    display: "flex",
    alignItems: "center",
    gap: 10,
    padding: "8px 16px",
    border: "1px solid rgba(16, 185, 129, 0.3)",
    borderRadius: 6,
    background: "rgba(16, 185, 129, 0.04)",
  },
  doneName: {
    color: "rgba(16, 185, 129, 0.8)",
    fontSize: 12,
    fontWeight: 500,
  },
  activeBadge: {
    color: "rgba(16, 185, 129, 0.7)",
    fontSize: 8,
    fontFamily: "var(--font-mono)",
    letterSpacing: 1,
    border: "1px solid rgba(16, 185, 129, 0.25)",
    borderRadius: 3,
    padding: "1px 6px",
  },
  errorCard: {
    display: "flex",
    alignItems: "center",
    gap: 10,
    padding: "8px 16px",
    border: "1px solid rgba(255, 100, 100, 0.3)",
    borderRadius: 6,
    background: "rgba(255, 100, 100, 0.04)",
    width: "100%",
  },
  errorLabel: {
    color: "rgba(255, 100, 100, 0.7)",
    fontSize: 9,
    fontFamily: "var(--font-mono)",
    letterSpacing: 1,
    flexShrink: 0,
  },
  errorMsg: {
    color: "rgba(255, 100, 100, 0.6)",
    fontSize: 11,
    lineHeight: "1.3",
  },
};
