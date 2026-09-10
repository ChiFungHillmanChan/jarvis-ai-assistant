import { FormEvent, useEffect, useState } from "react";
import { useWorkspaceValue } from "../../workspace/WorkspaceContext";
import type { CronJobView } from "../../lib/types";
import { createReviewedCron, getUpcomingRuns, previewCustomCron, type CronInterpretation } from "../../lib/commands";

interface ConversionFlowProps { onJobCreated: (job: CronJobView) => void; }

export default function ConversionFlow({ onJobCreated }: ConversionFlowProps) {
  const [description, setDescription] = useWorkspaceValue("automationDescription", "");
  const [preview, setPreview] = useWorkspaceValue<CronInterpretation | null>("automationPreview", null);
  const [upcoming, setUpcoming] = useState<string[]>([]);
  const [phase, setPhase] = useState<"idle" | "interpreting" | "saving">("idle");
  const [error, setError] = useState<string | null>(null);
  const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone || "Local time";

  useEffect(() => {
    let current = true;
    if (!preview) { setUpcoming([]); return; }
    getUpcomingRuns(preview.schedule, 3).then(values => { if (current) setUpcoming(values); }).catch(error => { if (current) setError(String(error)); });
    return () => { current = false; };
  }, [preview]);

  async function interpret(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const value = description.trim();
    if (!value || phase !== "idle") return;
    setPhase("interpreting"); setError(null); setPreview(null);
    try { setPreview(await previewCustomCron(value)); } catch (error) { setError(String(error)); } finally { setPhase("idle"); }
  }

  async function create() {
    if (!preview || phase !== "idle") return;
    setPhase("saving"); setError(null);
    try {
      const job = await createReviewedCron(preview);
      onJobCreated(job); setDescription(""); setPreview(null);
    } catch (error) { setError(String(error)); } finally { setPhase("idle"); }
  }

  return <section className="tool-panel automation-create">
    <div><h2>Create an automation</h2><p>Describe the task and timing, then review the interpreted schedule before saving.</p></div>
    <form onSubmit={interpret}><label htmlFor="automation-description">Schedule request</label><div><input id="automation-description" value={description} onChange={event => { setDescription(event.target.value); setPreview(null); setError(null); }} placeholder="Every weekday at 9am, sync my calendar" disabled={phase !== "idle"} /><button className="tool-button" disabled={phase !== "idle" || !description.trim()}>{phase === "interpreting" ? "Interpreting…" : "Review schedule"}</button></div></form>
    {preview && <div className="automation-preview"><div><strong>{preview.name}</strong><span>{preview.description}</span></div><dl><dt>Action</dt><dd>{preview.action_type.replace(/_/g, " ")}</dd><dt>Schedule</dt><dd><details><summary>View schedule expression</summary><code>{preview.schedule}</code></details></dd><dt>Timezone</dt><dd>{timezone}</dd><dt>Next runs</dt><dd>{upcoming.length ? upcoming.join(", ") : "Calculating…"}</dd></dl><div className="automation-preview__actions"><button className="tool-button tool-button--quiet" onClick={() => setPreview(null)} disabled={phase !== "idle"}>Edit request</button><button className="tool-button" onClick={create} disabled={phase !== "idle"}>{phase === "saving" ? "Creating…" : "Create automation"}</button></div></div>}
    {error && <p className="tool-alert tool-alert--error" role="alert">{error}</p>}
  </section>;
}
