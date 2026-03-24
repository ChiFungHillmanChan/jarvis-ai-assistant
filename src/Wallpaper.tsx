import { useState, useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import type { VoiceState } from "./lib/types";

interface Point3D {
  x: number;
  y: number;
  z: number;
}

const SPHERE_RADIUS = 320;
const CORE_RADIUS = 55;
const PARTICLE_COUNT = 300;
const TARGET_FPS = 15;
const FRAME_INTERVAL = 1000 / TARGET_FPS;

function project(p: Point3D, cx: number, cy: number, fov: number) {
  const camDist = fov * 1.0;
  const z = p.z + camDist;
  if (z <= 1) return { x: cx, y: cy, scale: 0 };
  const scale = 500 / z;
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
  return {
    x: r * Math.sin(phi) * Math.cos(theta),
    y: r * Math.cos(phi),
    z: r * Math.sin(phi) * Math.sin(theta),
  };
}

interface Particle {
  theta: number;
  phi: number;
  r: number;
  speed: number;
  size: number;
}

function voiceStateToActivity(state: VoiceState): number {
  if (state === "Speaking" || state === "WakeWordSpeaking") return 1.0;
  if (
    state === "Processing" ||
    state === "WakeWordDetected" ||
    state === "WakeWordProcessing" ||
    (typeof state === "object" && "ModelDownloading" in state)
  )
    return 0.7;
  if (state === "Listening" || state === "WakeWordListening") return 0.35;
  return 0.0;
}

export default function Wallpaper() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const animRef = useRef(0);
  const lastFrameTime = useRef(0);
  const time = useRef(0);
  const activityRef = useRef(0);
  const targetActivity = useRef(0);
  const particles = useRef<Particle[]>([]);
  const initialized = useRef(false);

  const [, setVoiceState] = useState<VoiceState>("Idle");

  useEffect(() => {
    const unlisten = listen<VoiceState>("voice-state", (event) => {
      setVoiceState(event.payload);
      targetActivity.current = voiceStateToActivity(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

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

    if (!initialized.current) {
      const pts: Particle[] = [];
      for (let i = 0; i < PARTICLE_COUNT; i++) {
        const theta = (i * 2.399) % (Math.PI * 2);
        const phi = Math.acos(1 - 2 * ((i * 0.618) % 1));
        pts.push({
          theta,
          phi,
          r: SPHERE_RADIUS * (0.5 + Math.random() * 0.5),
          speed: (0.0003 + Math.random() * 0.001) * (Math.random() > 0.5 ? 1 : -1),
          size: 0.8 + Math.random() * 1.2,
        });
      }
      particles.current = pts;
      initialized.current = true;
    }

    const ry = { current: 0 };
    const rx = { current: 0.2 };

    function animate(timestamp: number) {
      if (!canvas || !ctx) return;

      const elapsed = timestamp - lastFrameTime.current;
      if (elapsed < FRAME_INTERVAL) {
        animRef.current = requestAnimationFrame(animate);
        return;
      }
      lastFrameTime.current = timestamp - (elapsed % FRAME_INTERVAL);

      time.current += 0.008;
      const w = canvas.width,
        h = canvas.height;
      const cx = w / 2,
        cy = h / 2;
      const fov = 700;

      activityRef.current +=
        (targetActivity.current - activityRef.current) * 0.04;
      const act = activityRef.current;

      ry.current += 0.0015 + act * 0.003;
      rx.current += 0.0003;

      // Transparent background for wallpaper
      ctx.clearRect(0, 0, w, h);

      // Subtle ambient glow
      const ambGlow = ctx.createRadialGradient(cx, cy, 0, cx, cy, SPHERE_RADIUS * 2);
      ambGlow.addColorStop(0, `rgba(0, 50, 100, ${0.06 + act * 0.04})`);
      ambGlow.addColorStop(0.4, `rgba(0, 25, 50, ${0.03 + act * 0.02})`);
      ambGlow.addColorStop(1, "rgba(0, 0, 0, 0)");
      ctx.fillStyle = ambGlow;
      ctx.fillRect(0, 0, w, h);

      // Grid lines (latitude)
      for (let i = 1; i < 10; i++) {
        const phi = (i / 10) * Math.PI;
        const ringR = SPHERE_RADIUS * Math.sin(phi);
        const ringY = SPHERE_RADIUS * Math.cos(phi);
        ctx.beginPath();
        for (let s = 0; s <= 48; s++) {
          const theta = (s / 48) * Math.PI * 2;
          const p = transform(
            { x: ringR * Math.cos(theta), y: ringY, z: ringR * Math.sin(theta) },
            ry.current,
            rx.current,
          );
          const pr = project(p, cx, cy, fov);
          if (s === 0) ctx.moveTo(pr.x, pr.y);
          else ctx.lineTo(pr.x, pr.y);
        }
        ctx.strokeStyle = `rgba(0, 160, 255, ${0.025 + act * 0.01})`;
        ctx.lineWidth = 0.4;
        ctx.stroke();
      }

      // Grid lines (longitude)
      for (let i = 0; i < 12; i++) {
        const theta = (i / 12) * Math.PI * 2;
        ctx.beginPath();
        for (let s = 0; s <= 32; s++) {
          const phi = (s / 32) * Math.PI;
          let p = sphereToCart(theta, phi, SPHERE_RADIUS);
          p = transform(p, ry.current, rx.current);
          const pr = project(p, cx, cy, fov);
          if (s === 0) ctx.moveTo(pr.x, pr.y);
          else ctx.lineTo(pr.x, pr.y);
        }
        ctx.strokeStyle = `rgba(0, 160, 255, ${0.02 + act * 0.008})`;
        ctx.lineWidth = 0.3;
        ctx.stroke();
      }

      // Orbital rings
      const ringDefs = [
        { r: SPHERE_RADIUS * 1.15, tx: 0.5, tz: 0.2, spd: 0.3, op: 0.06 },
        { r: SPHERE_RADIUS * 0.8, tx: 1.2, tz: 0.6, spd: -0.25, op: 0.05 },
        { r: SPHERE_RADIUS * 1.3, tx: 0.1, tz: 1.0, spd: 0.2, op: 0.04 },
      ];
      for (const ring of ringDefs) {
        ctx.beginPath();
        for (let s = 0; s <= 64; s++) {
          const a = (s / 64) * Math.PI * 2;
          let p: Point3D = { x: Math.cos(a) * ring.r, y: Math.sin(a) * ring.r, z: 0 };
          p = rotateX(p, ring.tx);
          p = rotateZ(p, ring.tz);
          p = transform(p, ry.current, rx.current);
          const pr = project(p, cx, cy, fov);
          if (s === 0) ctx.moveTo(pr.x, pr.y);
          else ctx.lineTo(pr.x, pr.y);
        }
        ctx.strokeStyle = `rgba(0, 180, 255, ${ring.op + act * 0.02})`;
        ctx.lineWidth = 0.6;
        ctx.stroke();

        // Energy dot
        const da = time.current * ring.spd * (1 + act * 1.5);
        let dp: Point3D = { x: Math.cos(da) * ring.r, y: Math.sin(da) * ring.r, z: 0 };
        dp = rotateX(dp, ring.tx);
        dp = rotateZ(dp, ring.tz);
        dp = transform(dp, ry.current, rx.current);
        const dpr = project(dp, cx, cy, fov);
        const gs = 8 * dpr.scale;
        const dg = ctx.createRadialGradient(dpr.x, dpr.y, 0, dpr.x, dpr.y, gs);
        dg.addColorStop(0, `rgba(100, 220, 255, ${0.3 + act * 0.2})`);
        dg.addColorStop(1, "rgba(0, 180, 255, 0)");
        ctx.beginPath();
        ctx.arc(dpr.x, dpr.y, gs, 0, Math.PI * 2);
        ctx.fillStyle = dg;
        ctx.fill();
      }

      // Core
      const breathAmp = 0.06 + act * 0.14;
      const breathSpd = 2.0 + act * 2.5;
      const breath = 1 + Math.sin(time.current * breathSpd) * breathAmp;
      const cR = CORE_RADIUS * breath;
      for (let layer = 0; layer < 3; layer++) {
        const glowR = cR * (3 - layer);
        const glowBase = [0.08, 0.04, 0.02][layer];
        const glowAlpha = glowBase * (1 + act * 2.0);
        const cg = ctx.createRadialGradient(cx, cy, 0, cx, cy, glowR);
        cg.addColorStop(0, `rgba(80, 200, 255, ${glowAlpha})`);
        cg.addColorStop(1, "rgba(0, 60, 120, 0)");
        ctx.beginPath();
        ctx.arc(cx, cy, glowR, 0, Math.PI * 2);
        ctx.fillStyle = cg;
        ctx.fill();
      }

      // Wireframe core icosahedron
      const t2 = (1 + Math.sqrt(5)) / 2;
      const rawV: Point3D[] = [
        { x: -1, y: t2, z: 0 }, { x: 1, y: t2, z: 0 }, { x: -1, y: -t2, z: 0 }, { x: 1, y: -t2, z: 0 },
        { x: 0, y: -1, z: t2 }, { x: 0, y: 1, z: t2 }, { x: 0, y: -1, z: -t2 }, { x: 0, y: 1, z: -t2 },
        { x: t2, y: 0, z: -1 }, { x: t2, y: 0, z: 1 }, { x: -t2, y: 0, z: -1 }, { x: -t2, y: 0, z: 1 },
      ];
      const edges = [
        [0, 1], [0, 5], [0, 7], [0, 10], [0, 11], [1, 5], [1, 7], [1, 8], [1, 9],
        [2, 3], [2, 4], [2, 6], [2, 10], [2, 11], [3, 4], [3, 6], [3, 8], [3, 9],
        [4, 5], [4, 9], [4, 11], [5, 9], [5, 11], [6, 7], [6, 8], [6, 10],
        [7, 8], [7, 10], [8, 9], [10, 11],
      ];
      const coreRot = time.current * (0.25 + act * 0.6);
      ctx.strokeStyle = `rgba(100, 220, 255, ${0.15 + act * 0.08})`;
      ctx.lineWidth = 0.6;
      for (const [a, b] of edges) {
        const len = Math.sqrt(rawV[a].x ** 2 + rawV[a].y ** 2 + rawV[a].z ** 2);
        let pa = { x: (rawV[a].x / len) * cR, y: (rawV[a].y / len) * cR, z: (rawV[a].z / len) * cR };
        let pb = { x: (rawV[b].x / len) * cR, y: (rawV[b].y / len) * cR, z: (rawV[b].z / len) * cR };
        pa = rotateY(pa, coreRot);
        pa = rotateX(pa, coreRot * 0.7);
        pa = transform(pa, ry.current, rx.current);
        pb = rotateY(pb, coreRot);
        pb = rotateX(pb, coreRot * 0.7);
        pb = transform(pb, ry.current, rx.current);
        const pra = project(pa, cx, cy, fov);
        const prb = project(pb, cx, cy, fov);
        ctx.beginPath();
        ctx.moveTo(pra.x, pra.y);
        ctx.lineTo(prb.x, prb.y);
        ctx.stroke();
      }

      // Particles
      for (const p of particles.current) {
        p.theta += p.speed;
        const cart = sphereToCart(p.theta, p.phi, p.r);
        const tp = transform(cart, ry.current, rx.current);
        const pr = project(tp, cx, cy, fov);
        const sz = p.size * pr.scale * 1.2;
        if (sz < 0.15 || pr.scale <= 0) continue;

        ctx.beginPath();
        ctx.arc(pr.x, pr.y, sz, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(0, 160, 255, ${0.25 * pr.scale})`;
        ctx.fill();
      }

      // Energy pulses
      const pulseCount = 2 + Math.floor(act * 2);
      const pulseSpeed = 0.25 + act * 0.3;
      for (let i = 0; i < pulseCount; i++) {
        const phase = (time.current * pulseSpeed + i * (1 / pulseCount)) % 1;
        const pr2 = CORE_RADIUS + phase * (SPHERE_RADIUS * 1.1 - CORE_RADIUS);
        const alpha = (1 - phase) * 0.04;
        if (alpha > 0.003) {
          const ps = project({ x: 0, y: 0, z: 0 }, cx, cy, fov).scale;
          ctx.beginPath();
          ctx.arc(cx, cy, pr2 * ps, 0, Math.PI * 2);
          ctx.strokeStyle = `rgba(0, 200, 255, ${alpha})`;
          ctx.lineWidth = 1;
          ctx.stroke();
        }
      }

      // Scan line
      const scanY = (time.current * 40) % h;
      const scanGrad = ctx.createLinearGradient(0, scanY - 10, 0, scanY + 10);
      scanGrad.addColorStop(0, "rgba(0, 180, 255, 0)");
      scanGrad.addColorStop(0.5, `rgba(0, 180, 255, ${0.008 + act * 0.005})`);
      scanGrad.addColorStop(1, "rgba(0, 180, 255, 0)");
      ctx.fillStyle = scanGrad;
      ctx.fillRect(0, scanY - 10, w, 20);

      animRef.current = requestAnimationFrame(animate);
    }

    animRef.current = requestAnimationFrame(animate);

    return () => {
      cancelAnimationFrame(animRef.current);
      window.removeEventListener("resize", resize);
    };
  }, []);

  return (
    <canvas
      ref={canvasRef}
      style={{
        position: "fixed",
        top: 0,
        left: 0,
        width: "100%",
        height: "100%",
        background: "transparent",
      }}
    />
  );
}
