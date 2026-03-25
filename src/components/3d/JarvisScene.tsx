import { useEffect, useRef, useCallback, memo } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * JARVIS Interactive Data Sphere
 *
 * Each particle is a real data node (task, email, meeting, PR, note).
 * Click a node to zoom in with smooth animation.
 * AI interactions target specific nodes and the sphere rotates to them.
 */

interface Point3D { x: number; y: number; z: number; }

interface DataNode {
  id: string;
  type: "task" | "email" | "meeting" | "github" | "notion" | "cron" | "particle";
  label: string;
  sublabel?: string;
  urgent?: boolean;
  // Spherical coordinates
  theta: number;
  phi: number;
  r: number;
  // Animation
  speed: number;
  size: number;
  pulsePhase: number;
}

interface EnergyArc {
  targetIdx: number;
  progress: number;
  speed: number;
  trail: { x: number; y: number }[];
  color: { r: number; g: number; b: number };
  active: boolean;
  side: number;
}

const SPHERE_RADIUS = 280;
const CORE_RADIUS = 45;
const LAT_LINES = 12;
const LON_LINES = 16;
const NODE_HOVER_DIST = 18;

const TYPE_COLORS: Record<string, { r: number; g: number; b: number }> = {
  task:     { r: 0,   g: 180, b: 255 },  // cyan
  email:    { r: 100, g: 200, b: 255 },  // light cyan
  meeting:  { r: 255, g: 180, b: 0   },  // amber
  github:   { r: 16,  g: 185, b: 129 },  // green
  notion:   { r: 180, g: 130, b: 255 },  // purple
  cron:     { r: 0,   g: 220, b: 200 },  // teal
  particle: { r: 0,   g: 160, b: 255 },  // dim cyan
};

function project(p: Point3D, cx: number, cy: number, fov: number) {
  const camDist = fov * 1.0;
  const z = p.z + camDist;
  if (z <= 1) return { x: cx, y: cy, scale: 0 };
  const scale = 400 / z;
  return { x: cx + p.x * scale, y: cy + p.y * scale, scale: Math.max(scale, 0) };
}

function rotateY(p: Point3D, a: number): Point3D {
  const c = Math.cos(a), s = Math.sin(a);
  return { x: p.x * c - p.z * s, y: p.y, z: p.x * s + p.z * c };
}
function rotateX(p: Point3D, a: number): Point3D {
  const c = Math.cos(a), s = Math.sin(a);
  return { x: p.x, y: p.y * c - p.z * s, z: p.y * s + p.z * c };
}
function rotateZ(p: Point3D, a: number): Point3D {
  const c = Math.cos(a), s = Math.sin(a);
  return { x: p.x * c - p.y * s, y: p.x * s + p.y * c, z: p.z };
}
function transform(p: Point3D, ry: number, rx: number): Point3D {
  return rotateX(rotateY(p, ry), rx);
}
function sphereToCart(theta: number, phi: number, r: number): Point3D {
  return { x: r * Math.sin(phi) * Math.cos(theta), y: r * Math.cos(phi), z: r * Math.sin(phi) * Math.sin(theta) };
}

// Easing function
function easeInOut(t: number): number {
  return t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2;
}

interface JarvisSceneProps {
  activityLevel?: "idle" | "listening" | "processing" | "active";
  ttsAmplitudeRef?: React.RefObject<number>;
  micAmplitudeRef?: React.RefObject<number>;
  pendingToolCall?: string | null;
  onToolCallConsumed?: () => void;
}

export default memo(function JarvisScene({ activityLevel = "idle", ttsAmplitudeRef, micAmplitudeRef, pendingToolCall = null, onToolCallConsumed }: JarvisSceneProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const animRef = useRef(0);
  const time = useRef(0);
  const activityRef = useRef(0); // 0 = idle, 1 = max activity (smooth interpolation)
  const rotYRef = useRef(0);
  const rotXRef = useRef(0.25);
  const targetRY = useRef(0);
  const targetRX = useRef(0.25);
  const zoomRef = useRef(600);
  const targetZoom = useRef(600);
  const dragging = useRef(false);
  const lastM = useRef({ x: 0, y: 0 });
  const autoRot = useRef(true);
  const mousePos = useRef({ x: 0, y: 0 });
  const speakingAlpha = useRef(0);
  const ttsAmpFallback = useRef(0);
  const ttsAmpRef = ttsAmplitudeRef || ttsAmpFallback;
  const micAmpFallback = useRef(0);
  const micAmpRef = micAmplitudeRef || micAmpFallback;
  const stateColorRef = useRef({ r: 0, g: 180, b: 255 });
  const targetColorRef = useRef({ r: 0, g: 180, b: 255 });
  const listeningAlphaRef = useRef(0);
  const arcsRef = useRef<EnergyArc[]>([]);
  const ringSpeedRef = useRef(1.0);
  const gridOpacityRef = useRef(1);

  // Data nodes
  const nodes = useRef<DataNode[]>([]);
  const dataLoaded = useRef(false);

  // Z-sort cache (avoid sorting every frame)
  const sortedNodesRef = useRef<{ node: DataNode; pr: { x: number; y: number; scale: number }; tz: number }[]>([]);
  const lastSortRotXRef = useRef(0);
  const lastSortRotYRef = useRef(0);

  // Focus animation state
  const focusTarget = useRef<DataNode | null>(null);
  const focusProgress = useRef(0); // 0 to 1
  const focusStartRY = useRef(0);
  const focusStartRX = useRef(0);
  const focusStartZoom = useRef(600);
  const focusedNode = useRef<DataNode | null>(null);

  // Tooltip
  const hoveredNode = useRef<DataNode | null>(null);

  // Load real data from backend
  const loadData = useCallback(async () => {
    type FreshItem = { type: DataNode["type"]; label: string; sublabel?: string; urgent?: boolean };
    const freshItems: FreshItem[] = [];

    try {
      const tasks: { id: number; title: string; deadline: string | null; status: string; priority: number }[] = await invoke("get_tasks");
      for (const t of tasks.slice(0, 15)) {
        freshItems.push({ type: "task", label: t.title, sublabel: t.deadline ? `Due: ${t.deadline}` : undefined, urgent: t.priority >= 2 });
      }

      const emails: { id: number; subject: string | null; sender: string }[] = await invoke("get_emails", { limit: 10 });
      for (const e of emails.slice(0, 10)) {
        freshItems.push({ type: "email", label: e.subject || "(No subject)", sublabel: e.sender });
      }

      const events: { id: number; summary: string; start_time: string }[] = await invoke("get_todays_events");
      for (const ev of events.slice(0, 8)) {
        freshItems.push({ type: "meeting", label: ev.summary, sublabel: new Date(ev.start_time).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) });
      }

      const ghItems: { id: number; title: string; repo: string; item_type: string }[] = await invoke("get_github_items", { item_type: null });
      for (const g of ghItems.slice(0, 8)) {
        freshItems.push({ type: "github", label: g.title, sublabel: `${g.repo} [${g.item_type}]` });
      }

      const crons: { id: number; name: string; status: string; last_run: string | null }[] = await invoke("get_cron_jobs");
      for (const c of crons) {
        freshItems.push({ type: "cron", label: c.name, sublabel: c.status });
      }
    } catch {
      // Backend may not be ready yet -- particles only on initial load
    }

    // On refresh: update labels/urgency in-place, preserve all positions
    if (dataLoaded.current && nodes.current.length > 0) {
      const freshByType: Record<string, FreshItem[]> = {};
      for (const item of freshItems) {
        if (!freshByType[item.type]) freshByType[item.type] = [];
        freshByType[item.type].push(item);
      }

      const existingByType: Record<string, DataNode[]> = {};
      for (const node of nodes.current) {
        if (node.type === "particle") continue;
        if (!existingByType[node.type]) existingByType[node.type] = [];
        existingByType[node.type].push(node);
      }

      for (const type of Object.keys(existingByType)) {
        const existing = existingByType[type];
        const fresh = freshByType[type] || [];
        for (let i = 0; i < existing.length && i < fresh.length; i++) {
          existing[i].label = fresh[i].label;
          existing[i].sublabel = fresh[i].sublabel;
          existing[i].urgent = fresh[i].urgent;
        }
      }
      return;
    }

    // Initial load: create all nodes with positions
    const dataNodes: DataNode[] = [];
    let idx = 0;

    function addNode(type: DataNode["type"], label: string, sublabel?: string, urgent?: boolean) {
      const theta = (idx * 2.399) % (Math.PI * 2);
      const phi = Math.acos(1 - 2 * ((idx * 0.618) % 1));
      const layer = type === "particle" ? 0.6 + Math.random() * 0.4 : 0.85 + Math.random() * 0.15;
      dataNodes.push({
        id: `${type}-${idx}`,
        type, label, sublabel, urgent,
        theta, phi,
        r: SPHERE_RADIUS * layer,
        speed: (0.0005 + Math.random() * 0.002) * (Math.random() > 0.5 ? 1 : -1),
        size: type === "particle" ? 1 : 2.5 + Math.random() * 1.5,
        pulsePhase: Math.random() * Math.PI * 2,
      });
      idx++;
    }

    for (const item of freshItems) {
      addNode(item.type, item.label, item.sublabel, item.urgent);
    }

    const targetTotal = 60;
    while (idx < targetTotal) {
      addNode("particle", "", undefined);
    }

    nodes.current = dataNodes;
    dataLoaded.current = true;
  }, []);

  // Focus on a node -- smooth animation
  const focusOnNode = useCallback((node: DataNode) => {
    focusTarget.current = node;
    focusProgress.current = 0;
    focusStartRY.current = rotYRef.current;
    focusStartRX.current = rotXRef.current;
    focusStartZoom.current = zoomRef.current;
    autoRot.current = false;
  }, []);

  // ttsAmplitude is read directly from the ref passed by App.tsx (no re-render needed)
  // The animation loop reads ttsAmplitudeRef.current each frame.

  useEffect(() => {
    if (pendingToolCall && nodes.current.length > 0) {
      const name = pendingToolCall.toLowerCase();
      let nodeType = "task";
      if (name.includes("calendar") || name.includes("event")) nodeType = "meeting";
      else if (name.includes("email") || name.includes("gmail")) nodeType = "email";
      else if (name.includes("github") || name.includes("pr") || name.includes("issue")) nodeType = "github";
      else if (name.includes("notion")) nodeType = "notion";
      else if (name.includes("cron") || name.includes("schedule")) nodeType = "cron";
      else if (name.includes("note") || name.includes("obsidian")) nodeType = "notion";

      const candidates = nodes.current.filter(n => n.type === nodeType);
      const target = candidates.length > 0
        ? candidates[Math.floor(Math.random() * candidates.length)]
        : nodes.current[Math.floor(Math.random() * nodes.current.length)];

      const targetIdx = nodes.current.indexOf(target);
      const color = TYPE_COLORS[target.type] || TYPE_COLORS.task;

      arcsRef.current.push({
        targetIdx, progress: 0,
        speed: 0.025 + Math.random() * 0.015,
        trail: [], color, active: true,
        side: arcsRef.current.length % 2 === 0 ? 1 : -1,
      });
      onToolCallConsumed?.();
    }
  }, [pendingToolCall, onToolCallConsumed]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    function resize() {
      if (!canvas) return;
      canvas.width = window.innerWidth;
      canvas.height = window.innerHeight;
    }
    resize();
    window.addEventListener("resize", resize);

    // Load data after dashboard has had time to initialize
    setTimeout(() => loadData(), 3000);
    // Refresh data every 60 seconds (reduced from 30s to lower DB contention)
    const dataInterval = setInterval(() => loadData(), 60000);

    // Mouse controls
    function onDown(e: MouseEvent) {
      const target = e.target as HTMLElement;
      if (target.closest("button, input, select, textarea, .panel, .no-drag, nav, form")) return;
      e.preventDefault();
      document.body.style.userSelect = "none";
      document.body.style.webkitUserSelect = "none";

      // Check if clicking on a node
      if (hoveredNode.current && hoveredNode.current.type !== "particle") {
        focusOnNode(hoveredNode.current);
        return;
      }

      dragging.current = true;
      lastM.current = { x: e.clientX, y: e.clientY };
      autoRot.current = false;
      document.body.style.cursor = "grabbing";
    }
    function onMove(e: MouseEvent) {
      mousePos.current = { x: e.clientX, y: e.clientY };
      if (!dragging.current) return;
      e.preventDefault();
      targetRY.current += (e.clientX - lastM.current.x) * 0.005;
      targetRX.current += (e.clientY - lastM.current.y) * 0.005;
      targetRX.current = Math.max(-1.5, Math.min(1.5, targetRX.current));
      lastM.current = { x: e.clientX, y: e.clientY };
    }
    function onUp() {
      dragging.current = false;
      document.body.style.userSelect = "";
      document.body.style.webkitUserSelect = "";
      document.body.style.cursor = "";
      setTimeout(() => { if (!dragging.current && !focusTarget.current) autoRot.current = true; }, 2000);
    }
    function onWheel(e: WheelEvent) {
      e.preventDefault();
      targetZoom.current += e.deltaY * 0.8;
      targetZoom.current = Math.max(50, Math.min(2000, targetZoom.current));
      // If zooming out while focused, dismiss focus
      if (focusedNode.current && e.deltaY > 0) {
        focusedNode.current = null;
        focusTarget.current = null;
        targetZoom.current = 600;
        autoRot.current = true;
      }
    }
    function onDblClick() {
      // Double click to unfocus
      if (focusedNode.current) {
        focusedNode.current = null;
        focusTarget.current = null;
        targetZoom.current = 600;
        autoRot.current = true;
      }
    }

    window.addEventListener("mousedown", onDown);
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    window.addEventListener("wheel", onWheel, { passive: false });
    window.addEventListener("dblclick", onDblClick);

    // Frame rate throttle: ~30fps when idle (smooth rotation), 60fps when active
    let lastFrameTime = 0;
    const IDLE_FRAME_MS = 33;   // 30fps when idle -- smooth enough for auto-rotation
    const ACTIVE_FRAME_MS = 16; // 60fps when active

    function animate(timestamp?: number) {
      if (!canvas || !ctx) return;

      // Throttle frame rate based on activity
      const now = timestamp || performance.now();
      const isInteracting = dragging.current || focusTarget.current !== null || arcsRef.current.length > 0;
      const frameInterval = (activityLevel !== "idle" || isInteracting) ? ACTIVE_FRAME_MS : IDLE_FRAME_MS;
      if (now - lastFrameTime < frameInterval) {
        animRef.current = requestAnimationFrame(animate);
        return;
      }
      lastFrameTime = now;

      try { // --- begin frame ---
      time.current += 0.006;
      const w = canvas.width, h = canvas.height;
      const cx = w / 2, cy = h / 2;

      // Activity level interpolation (smooth lerp ~0.5s transition)
      const targetAct = activityLevel === "active" ? 1.0
        : activityLevel === "processing" ? 0.7
        : activityLevel === "listening" ? 0.4 : 0.0;
      activityRef.current += (targetAct - activityRef.current) * 0.03;
      const act = activityRef.current;

      // Speaking crossfade
      const isSpeaking = (ttsAmpRef.current ?? 0) > 0.01;
      const speakTarget = isSpeaking ? 1 : 0;
      const speakSpeed = isSpeaking ? 0.06 : 0.035;
      speakingAlpha.current += (speakTarget - speakingAlpha.current) * speakSpeed;
      const spkAlpha = speakingAlpha.current;

      // Ring speed multiplier
      const ringTarget = activityLevel === "processing" ? 3.0 : activityLevel === "listening" ? 1.2 : 1.0;
      const ringLerp = ringTarget > ringSpeedRef.current ? 0.05 : 0.03;
      ringSpeedRef.current += (ringTarget - ringSpeedRef.current) * ringLerp;
      const ringMult = ringSpeedRef.current;

      // Voice state color targets
      const colorTargets: Record<string, { r: number; g: number; b: number }> = {
        idle: { r: 0, g: 180, b: 255 },
        listening: { r: 0, g: 180, b: 255 },
        processing: { r: 255, g: 180, b: 0 },
        active: { r: 16, g: 185, b: 129 },
      };
      targetColorRef.current = colorTargets[activityLevel] || colorTargets.idle;
      const sc = stateColorRef.current;
      const tc = targetColorRef.current;
      const lerpSpeed = 0.04;
      sc.r += (tc.r - sc.r) * lerpSpeed;
      sc.g += (tc.g - sc.g) * lerpSpeed;
      sc.b += (tc.b - sc.b) * lerpSpeed;

      // Smooth zoom + rotation
      zoomRef.current += (targetZoom.current - zoomRef.current) * 0.08;
      const fov = zoomRef.current;

      // Focus animation
      if (focusTarget.current && focusProgress.current < 1) {
        focusProgress.current = Math.min(1, focusProgress.current + 0.015);
        const t = easeInOut(focusProgress.current);

        // Calculate target rotation to face the node
        const node = focusTarget.current;
        const targetTheta = -node.theta + Math.PI / 2;
        const targetPhi = -(node.phi - Math.PI / 2);

        targetRY.current = focusStartRY.current + (targetTheta - focusStartRY.current) * t;
        targetRX.current = focusStartRX.current + (targetPhi - focusStartRX.current) * t;
        targetZoom.current = focusStartZoom.current + (180 - focusStartZoom.current) * t;

        if (focusProgress.current >= 1) {
          focusedNode.current = focusTarget.current;
          focusTarget.current = null;
        }
      }

      if (autoRot.current) targetRY.current += 0.002 + act * 0.004;
      rotYRef.current += (targetRY.current - rotYRef.current) * 0.06;
      rotXRef.current += (targetRX.current - rotXRef.current) * 0.06;
      const ry = rotYRef.current, rx = rotXRef.current;

      // === BACKGROUND ===
      ctx.fillStyle = "#060a14";
      ctx.fillRect(0, 0, w, h);

      const ambGlow = ctx.createRadialGradient(cx, cy, 0, cx, cy, SPHERE_RADIUS * 1.8);
      ambGlow.addColorStop(0, "rgba(0, 60, 120, 0.15)");
      ambGlow.addColorStop(0.3, "rgba(0, 30, 60, 0.08)");
      ambGlow.addColorStop(1, "rgba(0, 0, 0, 0)");
      ctx.fillStyle = ambGlow;
      ctx.fillRect(0, 0, w, h);

      // === GRID LINES (batched paths, skipped when idle) ===
      const targetGridOpacity = (activityLevel !== "idle" || dragging.current) ? 1 : 0;
      gridOpacityRef.current += (targetGridOpacity - gridOpacityRef.current) * 0.05;

      if (gridOpacityRef.current > 0.01) {
        ctx.globalAlpha = gridOpacityRef.current;

        // Latitude -- batch all lines except equator into one path
        ctx.lineWidth = 0.5;
        ctx.beginPath();
        for (let i = 1; i < LAT_LINES; i++) {
          if (i === LAT_LINES / 2) continue; // equator drawn separately (brighter)
          const phi = (i / LAT_LINES) * Math.PI;
          const ringR = SPHERE_RADIUS * Math.sin(phi);
          const ringY = SPHERE_RADIUS * Math.cos(phi);
          for (let s = 0; s <= 32; s++) {
            const theta = (s / 32) * Math.PI * 2;
            const p = transform({ x: ringR * Math.cos(theta), y: ringY, z: ringR * Math.sin(theta) }, ry, rx);
            const pr = project(p, cx, cy, fov);
            if (s === 0) ctx.moveTo(pr.x, pr.y); else ctx.lineTo(pr.x, pr.y);
          }
        }
        ctx.strokeStyle = "rgba(0, 160, 255, 0.04)";
        ctx.stroke();

        // Equator (brighter)
        {
          const eqPhi = (LAT_LINES / 2 / LAT_LINES) * Math.PI;
          const eqR = SPHERE_RADIUS * Math.sin(eqPhi);
          const eqY = SPHERE_RADIUS * Math.cos(eqPhi);
          ctx.beginPath();
          for (let s = 0; s <= 32; s++) {
            const theta = (s / 32) * Math.PI * 2;
            const p = transform({ x: eqR * Math.cos(theta), y: eqY, z: eqR * Math.sin(theta) }, ry, rx);
            const pr = project(p, cx, cy, fov);
            if (s === 0) ctx.moveTo(pr.x, pr.y); else ctx.lineTo(pr.x, pr.y);
          }
          ctx.strokeStyle = "rgba(0, 160, 255, 0.08)";
          ctx.stroke();
        }

        // Longitude -- batch all into one path
        ctx.beginPath();
        for (let i = 0; i < LON_LINES; i++) {
          const theta = (i / LON_LINES) * Math.PI * 2;
          for (let s = 0; s <= 24; s++) {
            const phi = (s / 24) * Math.PI;
            let p = sphereToCart(theta, phi, SPHERE_RADIUS);
            p = transform(p, ry, rx);
            const pr = project(p, cx, cy, fov);
            if (s === 0) ctx.moveTo(pr.x, pr.y); else ctx.lineTo(pr.x, pr.y);
          }
        }
        ctx.strokeStyle = "rgba(0, 160, 255, 0.03)";
        ctx.lineWidth = 0.4;
        ctx.stroke();

        ctx.globalAlpha = 1;
      }

      // === ORBITAL RINGS ===
      const ringDefs = [
        { r: SPHERE_RADIUS * 1.1, tx: 0.5, tz: 0.2, spd: 0.4, op: 0.1 },
        { r: SPHERE_RADIUS * 0.75, tx: 1.2, tz: 0.6, spd: -0.3, op: 0.08 },
        { r: SPHERE_RADIUS * 1.25, tx: 0.1, tz: 1.0, spd: 0.25, op: 0.06 },
        { r: SPHERE_RADIUS * 0.55, tx: 0.8, tz: 0.3, spd: 0.55, op: 0.08 },
        { r: SPHERE_RADIUS * 1.05, tx: 0.7, tz: 1.3, spd: -0.45, op: 0.06 },
      ];
      for (const ring of ringDefs) {
        ctx.beginPath();
        for (let s = 0; s <= 48; s++) {
          const a = (s / 48) * Math.PI * 2;
          let p: Point3D = { x: Math.cos(a) * ring.r, y: Math.sin(a) * ring.r, z: 0 };
          p = rotateX(p, ring.tx); p = rotateZ(p, ring.tz); p = transform(p, ry, rx);
          const pr = project(p, cx, cy, fov);
          if (s === 0) ctx.moveTo(pr.x, pr.y); else ctx.lineTo(pr.x, pr.y);
        }
        ctx.strokeStyle = `rgba(0, 180, 255, ${ring.op})`;
        ctx.lineWidth = 0.8;
        ctx.stroke();

        // Energy dot (speed increases with activity)
        const da = time.current * ring.spd * ringMult;
        let dp: Point3D = { x: Math.cos(da) * ring.r, y: Math.sin(da) * ring.r, z: 0 };
        dp = rotateX(dp, ring.tx); dp = rotateZ(dp, ring.tz); dp = transform(dp, ry, rx);
        const dpr = project(dp, cx, cy, fov);
        const gs = 12 * dpr.scale;
        const dg = ctx.createRadialGradient(dpr.x, dpr.y, 0, dpr.x, dpr.y, gs);
        dg.addColorStop(0, "rgba(100, 220, 255, 0.5)");
        dg.addColorStop(1, "rgba(0, 180, 255, 0)");
        ctx.beginPath(); ctx.arc(dpr.x, dpr.y, gs, 0, Math.PI * 2); ctx.fillStyle = dg; ctx.fill();
        ctx.beginPath(); ctx.arc(dpr.x, dpr.y, 2 * dpr.scale, 0, Math.PI * 2);
        ctx.fillStyle = "rgba(180, 240, 255, 0.9)"; ctx.fill();
        if (ringMult > 1.5) {
          for (let ti = 1; ti <= 4; ti++) {
            const trailA = da - ti * 0.15 * ring.spd;
            let tp: Point3D = { x: Math.cos(trailA) * ring.r, y: Math.sin(trailA) * ring.r, z: 0 };
            tp = rotateX(tp, ring.tx); tp = rotateZ(tp, ring.tz); tp = transform(tp, ry, rx);
            const tpr = project(tp, cx, cy, fov);
            const trailAlpha = 0.9 - ti * 0.22;
            if (trailAlpha > 0) {
              ctx.beginPath(); ctx.arc(tpr.x, tpr.y, 1.5 * tpr.scale, 0, Math.PI * 2);
              ctx.fillStyle = `rgba(180, 240, 255, ${trailAlpha})`; ctx.fill();
            }
          }
        }
      }

      // === CORE === (modulated by activity level + voice amplitude)
      const micAmp = micAmpRef.current ?? 0;
      const ttsAmp = ttsAmpRef.current ?? 0;
      const voiceAmp = Math.max(micAmp, ttsAmp);
      const coreScale = 1 + voiceAmp * 0.2;
      const breathAmp = 0.08 + act * 0.17;
      const breathSpd = 2.5 + act * 3.0;
      const breath = 1 + Math.sin(time.current * breathSpd) * breathAmp;
      const cR = CORE_RADIUS * breath * (1 + spkAlpha * 0.15) * coreScale;
      for (let layer = 0; layer < 3; layer++) {
        const glowR = cR * (3 - layer);
        const glowBase = [0.1, 0.05, 0.025][layer];
        const glowAlpha = glowBase * (1 + act * 2.5);
        const coreR = Math.round(80 + (sc.r - 0) * 0.3);
        const coreG = Math.round(200 + (sc.g - 180) * 0.3);
        const coreB = Math.round(255 + (sc.b - 255) * 0.3);
        const cg = ctx.createRadialGradient(cx, cy, 0, cx, cy, glowR);
        cg.addColorStop(0, `rgba(${coreR}, ${coreG}, ${coreB}, ${glowAlpha})`);
        cg.addColorStop(1, `rgba(${Math.round(sc.r * 0.2)}, ${Math.round(sc.g * 0.33)}, ${Math.round(sc.b * 0.47)}, 0)`);
        ctx.beginPath(); ctx.arc(cx, cy, glowR, 0, Math.PI * 2); ctx.fillStyle = cg; ctx.fill();
      }
      // Wireframe core
      const t2 = (1 + Math.sqrt(5)) / 2;
      const rawV: Point3D[] = [
        {x:-1,y:t2,z:0},{x:1,y:t2,z:0},{x:-1,y:-t2,z:0},{x:1,y:-t2,z:0},
        {x:0,y:-1,z:t2},{x:0,y:1,z:t2},{x:0,y:-1,z:-t2},{x:0,y:1,z:-t2},
        {x:t2,y:0,z:-1},{x:t2,y:0,z:1},{x:-t2,y:0,z:-1},{x:-t2,y:0,z:1},
      ];
      const edges = [[0,1],[0,5],[0,7],[0,10],[0,11],[1,5],[1,7],[1,8],[1,9],[2,3],[2,4],[2,6],[2,10],[2,11],[3,4],[3,6],[3,8],[3,9],[4,5],[4,9],[4,11],[5,9],[5,11],[6,7],[6,8],[6,10],[7,8],[7,10],[8,9],[10,11]];
      const coreRot = time.current * (0.3 + act * 0.9);
      const coreAlpha = 0.2 * (1 - spkAlpha * 0.7);
      ctx.strokeStyle = `rgba(100, 220, 255, ${coreAlpha})`;
      ctx.lineWidth = 0.7;
      for (const [a, b] of edges) {
        const len = Math.sqrt(rawV[a].x**2 + rawV[a].y**2 + rawV[a].z**2);
        let pa = { x: rawV[a].x/len*cR, y: rawV[a].y/len*cR, z: rawV[a].z/len*cR };
        let pb = { x: rawV[b].x/len*cR, y: rawV[b].y/len*cR, z: rawV[b].z/len*cR };
        pa = rotateY(pa, coreRot); pa = rotateX(pa, coreRot*0.7); pa = transform(pa, ry, rx);
        pb = rotateY(pb, coreRot); pb = rotateX(pb, coreRot*0.7); pb = transform(pb, ry, rx);
        const pra = project(pa, cx, cy, fov), prb = project(pb, cx, cy, fov);
        ctx.beginPath(); ctx.moveTo(pra.x, pra.y); ctx.lineTo(prb.x, prb.y); ctx.stroke();
      }

      // === RADIAL WAVEFORM (speaking + listening states) ===
      // Listening alpha crossfade
      const isListening = activityLevel === "listening";
      listeningAlphaRef.current += (isListening ? 0.06 : -0.035);
      listeningAlphaRef.current = Math.max(0, Math.min(1, listeningAlphaRef.current));

      // Combined wave alpha: show waveform for either speaking OR listening
      const waveAlpha = Math.max(spkAlpha, listeningAlphaRef.current);

      // Choose amplitude source based on state
      const waveAmplitude = isListening ? micAmp : ttsAmp;

      if (waveAlpha > 0.01) {
        const barCount = 48;
        const innerR = 18;
        const maxOuterR = 55;

        ctx.strokeStyle = `rgba(${Math.round(sc.r)}, ${Math.round(sc.g)}, ${Math.round(sc.b)}, ${0.3 * waveAlpha})`;
        ctx.lineWidth = 1;
        ctx.beginPath(); ctx.arc(cx, cy, innerR, 0, Math.PI * 2); ctx.stroke();

        ctx.fillStyle = `rgba(${Math.round(sc.r)}, ${Math.round(sc.g)}, ${Math.round(sc.b)}, ${0.9 * waveAlpha})`;
        ctx.beginPath(); ctx.arc(cx, cy, 3, 0, Math.PI * 2); ctx.fill();

        for (let bi = 0; bi < barCount; bi++) {
          const angle = (bi / barCount) * Math.PI * 2;
          const barAmp = waveAmplitude * (0.3 + 0.7 * (
            0.5 * Math.sin(time.current * 0.07 + bi * 0.5) +
            0.3 * Math.sin(time.current * 0.11 + bi * 0.8) +
            0.2 * Math.sin(time.current * 0.15 + bi * 1.3)
          ));
          const norm = Math.max(0, Math.min(1, (barAmp + 1) / 2));
          const barLen = innerR + norm * (maxOuterR - innerR);

          const x1 = cx + Math.cos(angle) * innerR;
          const y1 = cy + Math.sin(angle) * innerR;
          const x2 = cx + Math.cos(angle) * barLen;
          const y2 = cy + Math.sin(angle) * barLen;

          const barColor = `rgba(${Math.round(sc.r)}, ${Math.round(sc.g)}, ${Math.round(sc.b)}, ${(0.4 + norm * 0.5) * waveAlpha})`;
          ctx.strokeStyle = barColor;
          ctx.lineWidth = 2.5;
          ctx.beginPath(); ctx.moveTo(x1, y1); ctx.lineTo(x2, y2); ctx.stroke();

          ctx.fillStyle = `rgba(${Math.round(sc.r)}, ${Math.round(sc.g)}, ${Math.round(sc.b)}, ${norm * 0.9 * waveAlpha})`;
          ctx.beginPath(); ctx.arc(x2, y2, 2, 0, Math.PI * 2); ctx.fill();
        }
      }

      // === DATA NODES ===
      hoveredNode.current = null;
      const mx = mousePos.current.x, my = mousePos.current.y;

      // Transform + sort back to front (sort cached when rotation unchanged)
      const transformed = nodes.current.map(node => {
        node.theta += node.speed;
        const cart = sphereToCart(node.theta, node.phi, node.r);

        // Particle attraction during listening: nudge toward center
        if (isListening && micAmp > 0.01) {
          const attractStrength = 0.3 * micAmp;
          cart.x *= (1 - attractStrength);
          cart.y *= (1 - attractStrength);
          cart.z *= (1 - attractStrength);
        }

        const tp = transform(cart, ry, rx);
        const pr = project(tp, cx, cy, fov);
        return { node, pr, tz: tp.z };
      });

      const rotChanged = Math.abs(ry - lastSortRotYRef.current) > 0.001 ||
                          Math.abs(rx - lastSortRotXRef.current) > 0.001;
      if (rotChanged || sortedNodesRef.current.length !== transformed.length) {
        transformed.sort((a, b) => b.tz - a.tz);
        sortedNodesRef.current = transformed;
        lastSortRotYRef.current = ry;
        lastSortRotXRef.current = rx;
      }
      const sorted = sortedNodesRef.current.length === transformed.length ? sortedNodesRef.current : transformed;

      // Draw connections between nearby non-particle nodes (spatial grid lookup)
      if (act > 0.05 || isInteracting) {
        const dataNodes = transformed.filter(t => t.node.type !== "particle");
        const CELL_SIZE = 80;
        const grid: Map<string, typeof dataNodes> = new Map();
        for (const dn of dataNodes) {
          const cellX = Math.floor(dn.pr.x / CELL_SIZE);
          const cellY = Math.floor(dn.pr.y / CELL_SIZE);
          const key = `${cellX},${cellY}`;
          if (!grid.has(key)) grid.set(key, []);
          grid.get(key)!.push(dn);
        }

        ctx.lineWidth = 0.3;
        ctx.beginPath();
        let hasLines = false;
        for (const dn of dataNodes) {
          const cellX = Math.floor(dn.pr.x / CELL_SIZE);
          const cellY = Math.floor(dn.pr.y / CELL_SIZE);
          for (let ddx = -1; ddx <= 1; ddx++) {
            for (let ddy = -1; ddy <= 1; ddy++) {
              const neighbors = grid.get(`${cellX + ddx},${cellY + ddy}`);
              if (!neighbors) continue;
              for (const other of neighbors) {
                if (other.node.id <= dn.node.id) continue; // avoid duplicates
                const dx = dn.pr.x - other.pr.x;
                const dy = dn.pr.y - other.pr.y;
                const distSq = dx * dx + dy * dy;
                if (distSq < 6400) { // 80*80
                  ctx.moveTo(dn.pr.x, dn.pr.y);
                  ctx.lineTo(other.pr.x, other.pr.y);
                  hasLines = true;
                }
              }
            }
          }
        }
        if (hasLines) {
          ctx.strokeStyle = "rgba(0, 180, 255, 0.06)";
          ctx.stroke();
        }
      }

      // Draw nodes (back-to-front using cached sort order)
      for (const { node, pr } of sorted) {
        const col = TYPE_COLORS[node.type] || TYPE_COLORS.particle;
        const sz = node.size * pr.scale * 1.5;
        if (sz < 0.2 || pr.scale <= 0) continue;

        const pulse = node.type !== "particle" ? 1 + Math.sin(time.current * 2 + node.pulsePhase) * 0.15 : 1;

        const flashAlpha = (node as any)._flashAlpha || 0;
        const flashScale = (node as any)._flashScale || 1;
        if (flashAlpha > 0.01) {
          (node as any)._flashAlpha *= 0.96;
          (node as any)._flashScale += (1 - (node as any)._flashScale) * 0.08;
        }
        const finalSz = sz * pulse * flashScale;

        // Check hover
        const dx = mx - pr.x, dy = my - pr.y;
        const isHovered = node.type !== "particle" && Math.sqrt(dx*dx + dy*dy) < NODE_HOVER_DIST * pr.scale;
        const isFocused = focusedNode.current?.id === node.id;

        if (isHovered) hoveredNode.current = node;

        // Glow for data nodes (only use gradient for hovered/focused, simple fill otherwise)
        if (node.type !== "particle") {
          if (isHovered || isFocused) {
            const glowSize = finalSz * (isHovered ? 6 : 8);
            const glowAlpha = (isHovered ? 0.15 : 0.2) * pr.scale;
            const ng = ctx.createRadialGradient(pr.x, pr.y, 0, pr.x, pr.y, glowSize);
            ng.addColorStop(0, `rgba(${col.r}, ${col.g}, ${col.b}, ${glowAlpha})`);
            ng.addColorStop(1, `rgba(${col.r}, ${col.g}, ${col.b}, 0)`);
            ctx.beginPath(); ctx.arc(pr.x, pr.y, glowSize, 0, Math.PI * 2); ctx.fillStyle = ng; ctx.fill();
          } else if (pr.scale > 0.4) {
            const glowSize = finalSz * 4;
            ctx.beginPath(); ctx.arc(pr.x, pr.y, glowSize, 0, Math.PI * 2);
            ctx.fillStyle = `rgba(${col.r}, ${col.g}, ${col.b}, ${0.03 * pr.scale})`;
            ctx.fill();
          }
        }

        // Urgent ring
        if (node.urgent) {
          ctx.beginPath();
          ctx.arc(pr.x, pr.y, finalSz * 2.5, 0, Math.PI * 2);
          ctx.strokeStyle = `rgba(255, 100, 100, ${0.3 + Math.sin(time.current * 3) * 0.15})`;
          ctx.lineWidth = 1;
          ctx.stroke();
        }

        // Node dot
        ctx.beginPath();
        ctx.arc(pr.x, pr.y, finalSz, 0, Math.PI * 2);
        const nodeAlpha = node.type === "particle" ? 0.3 * pr.scale : (isHovered ? 1 : 0.8) * pr.scale;
        ctx.fillStyle = `rgba(${col.r}, ${col.g}, ${col.b}, ${nodeAlpha})`;
        ctx.fill();

        // Flash glow ring (energy arc arrival effect)
        if (flashAlpha > 0.1) {
          ctx.strokeStyle = `rgba(${col.r}, ${col.g}, ${col.b}, ${flashAlpha * 0.5})`;
          ctx.lineWidth = 1.5;
          ctx.beginPath(); ctx.arc(pr.x, pr.y, finalSz + 6 * flashAlpha, 0, Math.PI * 2); ctx.stroke();
        }

        // Hover ring
        if (isHovered) {
          ctx.beginPath();
          ctx.arc(pr.x, pr.y, finalSz * 3, 0, Math.PI * 2);
          ctx.strokeStyle = `rgba(${col.r}, ${col.g}, ${col.b}, 0.5)`;
          ctx.lineWidth = 1;
          ctx.stroke();
        }

        // Label for data nodes (only when close enough or hovered/focused)
        if (node.type !== "particle" && node.label && (pr.scale > 0.5 || isHovered || isFocused)) {
          const labelAlpha = isHovered || isFocused ? 0.9 : Math.min(pr.scale * 0.8, 0.6);
          const fontSize = isHovered || isFocused ? 11 : Math.max(8, 10 * pr.scale);

          ctx.font = `${fontSize}px "JetBrains Mono", monospace`;
          ctx.fillStyle = `rgba(${col.r}, ${col.g}, ${col.b}, ${labelAlpha})`;
          ctx.textAlign = "left";

          const labelX = pr.x + finalSz + 6;
          const labelY = pr.y;

          // Type tag
          ctx.font = `bold ${fontSize * 0.7}px "JetBrains Mono", monospace`;
          ctx.fillText(node.type.toUpperCase(), labelX, labelY - fontSize * 0.4);

          // Title
          ctx.font = `${fontSize}px "JetBrains Mono", monospace`;
          const maxChars = isFocused ? 40 : isHovered ? 30 : 20;
          const title = node.label.length > maxChars ? node.label.slice(0, maxChars) + "..." : node.label;
          ctx.fillText(title, labelX, labelY + fontSize * 0.4);

          // Sublabel
          if (node.sublabel && (isHovered || isFocused)) {
            ctx.font = `${fontSize * 0.8}px "JetBrains Mono", monospace`;
            ctx.fillStyle = `rgba(${col.r}, ${col.g}, ${col.b}, ${labelAlpha * 0.6})`;
            ctx.fillText(node.sublabel, labelX, labelY + fontSize * 1.2);
          }
        }

        // Focused node -- draw expanded detail box
        if (isFocused && pr.scale > 0.3) {
          const boxX = pr.x + finalSz + 20;
          const boxY = pr.y - 30;
          const boxW = 220;
          const boxH = 60;

          ctx.fillStyle = `rgba(10, 14, 26, 0.85)`;
          ctx.strokeStyle = `rgba(${col.r}, ${col.g}, ${col.b}, 0.3)`;
          ctx.lineWidth = 1;
          ctx.beginPath();
          ctx.roundRect(boxX, boxY, boxW, boxH, 6);
          ctx.fill();
          ctx.stroke();

          // Connection line from node to box
          ctx.beginPath();
          ctx.moveTo(pr.x + finalSz, pr.y);
          ctx.lineTo(boxX, boxY + boxH / 2);
          ctx.strokeStyle = `rgba(${col.r}, ${col.g}, ${col.b}, 0.2)`;
          ctx.stroke();

          ctx.font = "bold 9px 'JetBrains Mono', monospace";
          ctx.fillStyle = `rgba(${col.r}, ${col.g}, ${col.b}, 0.6)`;
          ctx.fillText(node.type.toUpperCase(), boxX + 10, boxY + 16);

          ctx.font = "12px 'JetBrains Mono', monospace";
          ctx.fillStyle = `rgba(${col.r}, ${col.g}, ${col.b}, 0.9)`;
          const boxTitle = node.label.length > 28 ? node.label.slice(0, 28) + "..." : node.label;
          ctx.fillText(boxTitle, boxX + 10, boxY + 32);

          if (node.sublabel) {
            ctx.font = "10px 'JetBrains Mono', monospace";
            ctx.fillStyle = `rgba(${col.r}, ${col.g}, ${col.b}, 0.5)`;
            ctx.fillText(node.sublabel, boxX + 10, boxY + 48);
          }
        }
      }

      // === ENERGY PULSES === (more + faster when active)
      const pulseCount = 2 + Math.floor(act * 3);
      const pulseSpeed = 0.3 + act * 0.4;
      for (let i = 0; i < pulseCount; i++) {
        const phase = (time.current * pulseSpeed + i * (1 / pulseCount)) % 1;
        const pr2 = CORE_RADIUS + phase * (SPHERE_RADIUS * 1.1 - CORE_RADIUS);
        const alpha = (1 - phase) * 0.06;
        if (alpha > 0.005) {
          const ps = project({ x: 0, y: 0, z: 0 }, cx, cy, fov).scale;
          ctx.beginPath(); ctx.arc(cx, cy, pr2 * ps, 0, Math.PI * 2);
          ctx.strokeStyle = `rgba(0, 200, 255, ${alpha})`; ctx.lineWidth = 1.5; ctx.stroke();
        }
      }

      // === LEGEND (bottom left) ===
      if (zoomRef.current > 400) {
        const legendX = 20, legendY = h - 120;
        ctx.font = "9px 'JetBrains Mono', monospace";
        const types: DataNode["type"][] = ["task", "email", "meeting", "github", "notion", "cron"];
        types.forEach((type, i) => {
          const col = TYPE_COLORS[type];
          ctx.beginPath();
          ctx.arc(legendX + 6, legendY + i * 16, 4, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(${col.r}, ${col.g}, ${col.b}, 0.8)`;
          ctx.fill();
          ctx.fillStyle = `rgba(${col.r}, ${col.g}, ${col.b}, 0.5)`;
          ctx.fillText(type.toUpperCase(), legendX + 16, legendY + i * 16 + 3);
        });
      }

      // === ENERGY ARCS ===
      const arcs = arcsRef.current;
      for (let ai = arcs.length - 1; ai >= 0; ai--) {
        const arc = arcs[ai];
        if (!arc.active) { arcs.splice(ai, 1); continue; }

        const targetNode = nodes.current[arc.targetIdx];
        if (!targetNode) { arcs.splice(ai, 1); continue; }

        const tCart = sphereToCart(targetNode.theta, targetNode.phi, targetNode.r);
        const tRot = transform(tCart, ry, rx);
        const tProj = project(tRot, cx, cy, fov);

        arc.progress += arc.speed;
        const prog = Math.min(1, arc.progress);
        const eased = 1 - Math.pow(1 - prog, 3);

        const midX = cx + (tProj.x - cx) * 0.5 + Math.sin(prog * Math.PI) * 40 * arc.side;
        const midY = cy + (tProj.y - cy) * 0.5 - Math.sin(prog * Math.PI) * 30;
        const headX = (1-eased)*(1-eased)*cx + 2*(1-eased)*eased*midX + eased*eased*tProj.x;
        const headY = (1-eased)*(1-eased)*cy + 2*(1-eased)*eased*midY + eased*eased*tProj.y;

        arc.trail.push({ x: headX, y: headY });
        if (arc.trail.length > 15) arc.trail.shift();

        for (let ti = 1; ti < arc.trail.length; ti++) {
          const trailAlpha = (ti / arc.trail.length) * 0.7;
          const trailWidth = (ti / arc.trail.length) * 2.5;
          ctx.strokeStyle = `rgba(${arc.color.r}, ${arc.color.g}, ${arc.color.b}, ${trailAlpha})`;
          ctx.lineWidth = trailWidth;
          ctx.beginPath();
          ctx.moveTo(arc.trail[ti-1].x, arc.trail[ti-1].y);
          ctx.lineTo(arc.trail[ti].x, arc.trail[ti].y);
          ctx.stroke();
        }

        if (prog < 1) {
          const hg = ctx.createRadialGradient(headX, headY, 0, headX, headY, 8);
          hg.addColorStop(0, `rgba(${arc.color.r}, ${arc.color.g}, ${arc.color.b}, 0.9)`);
          hg.addColorStop(1, `rgba(${arc.color.r}, ${arc.color.g}, ${arc.color.b}, 0)`);
          ctx.beginPath(); ctx.arc(headX, headY, 8, 0, Math.PI * 2);
          ctx.fillStyle = hg; ctx.fill();
        }

        if (prog >= 1) {
          arc.active = false;
          (targetNode as any)._flashAlpha = 1.0;
          (targetNode as any)._flashScale = 1.8;
        }
      }

      // Scan line
      const scanY = ((time.current * 60) % h);
      const scanGrad = ctx.createLinearGradient(0, scanY - 15, 0, scanY + 15);
      scanGrad.addColorStop(0, "rgba(0, 180, 255, 0)");
      scanGrad.addColorStop(0.5, "rgba(0, 180, 255, 0.015)");
      scanGrad.addColorStop(1, "rgba(0, 180, 255, 0)");
      ctx.fillStyle = scanGrad;
      ctx.fillRect(0, scanY - 15, w, 30);

      // Cursor hint
      if (hoveredNode.current && hoveredNode.current.type !== "particle") {
        canvas.style.cursor = "pointer";
      } else {
        canvas.style.cursor = "";
      }

      } catch (e) {
        console.error("[JarvisScene] Animation error:", e);
      }
      animRef.current = requestAnimationFrame(animate);
    }

    animRef.current = requestAnimationFrame(animate);
    return () => {
      cancelAnimationFrame(animRef.current);
      clearInterval(dataInterval);
      window.removeEventListener("resize", resize);
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      window.removeEventListener("wheel", onWheel);
      window.removeEventListener("dblclick", onDblClick);
    };
  }, [loadData, focusOnNode]);

  return (
    <canvas ref={canvasRef} style={{
      position: "fixed", top: 0, left: 0, width: "100%", height: "100%", zIndex: 1,
    }} />
  );
})
