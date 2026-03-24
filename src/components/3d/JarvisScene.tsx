import { useEffect, useRef } from "react";

/**
 * JARVIS 3D Holographic Scene -- Canvas2D with perspective projection.
 * Full 3D rotation with mouse drag. Particles, orbital rings, core sphere,
 * energy pulses, and connection network.
 */

interface Point3D { x: number; y: number; z: number; }
interface Particle extends Point3D {
  vx: number; vy: number; vz: number;
  radius: number; opacity: number;
  trail: { x: number; y: number; alpha: number }[];
}

const PARTICLE_COUNT = 200;
const FIELD_RADIUS = 300;
const CORE_RADIUS = 50;
const CONNECTION_DIST = 90;
const RING_COUNT = 5;

function project(p: Point3D, cx: number, cy: number, fov: number): { x: number; y: number; scale: number } {
  const z = p.z + 600; // camera distance
  const scale = fov / (fov + z);
  return { x: cx + p.x * scale, y: cy + p.y * scale, scale };
}

function rotateY(p: Point3D, angle: number): Point3D {
  const cos = Math.cos(angle);
  const sin = Math.sin(angle);
  return { x: p.x * cos - p.z * sin, y: p.y, z: p.x * sin + p.z * cos };
}

function rotateX(p: Point3D, angle: number): Point3D {
  const cos = Math.cos(angle);
  const sin = Math.sin(angle);
  return { x: p.x, y: p.y * cos - p.z * sin, z: p.y * sin + p.z * cos };
}

export default function JarvisScene() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const particles = useRef<Particle[]>([]);
  const animRef = useRef<number>(0);
  const time = useRef(0);
  const rotY = useRef(0);
  const rotX = useRef(0.3);
  const targetRotY = useRef(0);
  const targetRotX = useRef(0.3);
  const dragging = useRef(false);
  const lastMouse = useRef({ x: 0, y: 0 });
  const autoRotate = useRef(true);

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

    // Mouse drag for 3D rotation
    function onMouseDown(e: MouseEvent) {
      dragging.current = true;
      lastMouse.current = { x: e.clientX, y: e.clientY };
      autoRotate.current = false;
    }
    function onMouseMove(e: MouseEvent) {
      if (!dragging.current) return;
      const dx = e.clientX - lastMouse.current.x;
      const dy = e.clientY - lastMouse.current.y;
      targetRotY.current += dx * 0.005;
      targetRotX.current += dy * 0.005;
      targetRotX.current = Math.max(-1.2, Math.min(1.2, targetRotX.current));
      lastMouse.current = { x: e.clientX, y: e.clientY };
    }
    function onMouseUp() {
      dragging.current = false;
      // Resume auto-rotate after 3 seconds
      setTimeout(() => { if (!dragging.current) autoRotate.current = true; }, 3000);
    }
    canvas.addEventListener("mousedown", onMouseDown);
    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);

    // Init particles in a sphere
    particles.current = [];
    for (let i = 0; i < PARTICLE_COUNT; i++) {
      const theta = Math.random() * Math.PI * 2;
      const phi = Math.acos(2 * Math.random() - 1);
      const r = CORE_RADIUS + 30 + Math.random() * (FIELD_RADIUS - CORE_RADIUS - 30);
      particles.current.push({
        x: r * Math.sin(phi) * Math.cos(theta),
        y: r * Math.sin(phi) * Math.sin(theta),
        z: r * Math.cos(phi),
        vx: (Math.random() - 0.5) * 0.3,
        vy: (Math.random() - 0.5) * 0.3,
        vz: (Math.random() - 0.5) * 0.3,
        radius: 0.8 + Math.random() * 1.5,
        opacity: 0.3 + Math.random() * 0.5,
        trail: [],
      });
    }

    function animate() {
      if (!canvas || !ctx) return;
      time.current += 0.008;
      const w = canvas.width;
      const h = canvas.height;
      const cx = w / 2;
      const cy = h / 2;
      const fov = 500;

      // Smooth rotation
      if (autoRotate.current) {
        targetRotY.current += 0.003;
      }
      rotY.current += (targetRotY.current - rotY.current) * 0.05;
      rotX.current += (targetRotX.current - rotX.current) * 0.05;

      // Clear with dark bg
      ctx.fillStyle = "#0a0e1a";
      ctx.fillRect(0, 0, w, h);

      // Radial glow in center
      const bgGlow = ctx.createRadialGradient(cx, cy, 0, cx, cy, 350);
      bgGlow.addColorStop(0, "rgba(0, 40, 80, 0.3)");
      bgGlow.addColorStop(0.4, "rgba(0, 20, 50, 0.1)");
      bgGlow.addColorStop(1, "rgba(0, 0, 0, 0)");
      ctx.fillStyle = bgGlow;
      ctx.fillRect(0, 0, w, h);

      // === DRAW ORBITAL RINGS ===
      for (let r = 0; r < RING_COUNT; r++) {
        const ringR = CORE_RADIUS + 40 + r * 55;
        const tiltX = 0.2 + r * 0.25;
        const tiltZ = r * 0.4;
        const segments = 80;
        const ringOpacity = 0.12 - r * 0.015;

        ctx.beginPath();
        for (let s = 0; s <= segments; s++) {
          const a = (s / segments) * Math.PI * 2;
          let p: Point3D = {
            x: Math.cos(a) * ringR,
            y: Math.sin(a) * ringR * Math.cos(tiltX),
            z: Math.sin(a) * ringR * Math.sin(tiltX),
          };
          // Apply tilt rotation
          const cz = Math.cos(tiltZ), sz = Math.sin(tiltZ);
          const nx = p.x * cz - p.y * sz;
          const ny = p.x * sz + p.y * cz;
          p = { x: nx, y: ny, z: p.z };

          p = rotateY(p, rotY.current);
          p = rotateX(p, rotX.current);
          const proj = project(p, cx, cy, fov);
          if (s === 0) ctx.moveTo(proj.x, proj.y);
          else ctx.lineTo(proj.x, proj.y);
        }
        ctx.strokeStyle = `rgba(0, 180, 255, ${ringOpacity})`;
        ctx.lineWidth = 1;
        ctx.stroke();

        // Orbiting energy dot
        const dotAngle = time.current * (0.5 + r * 0.15) + r * 1.5;
        let dotP: Point3D = {
          x: Math.cos(dotAngle) * ringR,
          y: Math.sin(dotAngle) * ringR * Math.cos(tiltX),
          z: Math.sin(dotAngle) * ringR * Math.sin(tiltX),
        };
        const cz2 = Math.cos(tiltZ), sz2 = Math.sin(tiltZ);
        dotP = { x: dotP.x * cz2 - dotP.y * sz2, y: dotP.x * sz2 + dotP.y * cz2, z: dotP.z };
        dotP = rotateY(dotP, rotY.current);
        dotP = rotateX(dotP, rotX.current);
        const dotProj = project(dotP, cx, cy, fov);

        // Energy dot glow
        const gSize = 15 * dotProj.scale;
        const dotGlow = ctx.createRadialGradient(dotProj.x, dotProj.y, 0, dotProj.x, dotProj.y, gSize);
        dotGlow.addColorStop(0, `rgba(0, 200, 255, 0.5)`);
        dotGlow.addColorStop(0.4, `rgba(0, 180, 255, 0.15)`);
        dotGlow.addColorStop(1, "rgba(0, 180, 255, 0)");
        ctx.beginPath();
        ctx.arc(dotProj.x, dotProj.y, gSize, 0, Math.PI * 2);
        ctx.fillStyle = dotGlow;
        ctx.fill();

        ctx.beginPath();
        ctx.arc(dotProj.x, dotProj.y, 2.5 * dotProj.scale, 0, Math.PI * 2);
        ctx.fillStyle = "rgba(100, 220, 255, 0.9)";
        ctx.fill();
      }

      // === DRAW CORE SPHERE (wireframe icosahedron) ===
      const coreBreath = 1 + Math.sin(time.current * 2) * 0.06;
      const cR = CORE_RADIUS * coreBreath;

      // Generate icosahedron vertices
      const t2 = (1 + Math.sqrt(5)) / 2;
      const icoVerts: Point3D[] = [
        {x:-1,y:t2,z:0},{x:1,y:t2,z:0},{x:-1,y:-t2,z:0},{x:1,y:-t2,z:0},
        {x:0,y:-1,z:t2},{x:0,y:1,z:t2},{x:0,y:-1,z:-t2},{x:0,y:1,z:-t2},
        {x:t2,y:0,z:-1},{x:t2,y:0,z:1},{x:-t2,y:0,z:-1},{x:-t2,y:0,z:1},
      ].map(v => {
        const len = Math.sqrt(v.x*v.x + v.y*v.y + v.z*v.z);
        return { x: v.x/len * cR, y: v.y/len * cR, z: v.z/len * cR };
      });
      const icoEdges = [
        [0,1],[0,5],[0,7],[0,10],[0,11],[1,5],[1,7],[1,8],[1,9],
        [2,3],[2,4],[2,6],[2,10],[2,11],[3,4],[3,6],[3,8],[3,9],
        [4,5],[4,9],[4,11],[5,9],[5,11],[6,7],[6,8],[6,10],
        [7,8],[7,10],[8,9],[10,11],
      ];

      const coreRot = time.current * 0.3;
      ctx.strokeStyle = `rgba(0, 180, 255, 0.25)`;
      ctx.lineWidth = 0.8;
      for (const [a, b] of icoEdges) {
        let pa = rotateY(icoVerts[a], coreRot);
        pa = rotateX(pa, coreRot * 0.7);
        pa = rotateY(pa, rotY.current);
        pa = rotateX(pa, rotX.current);

        let pb = rotateY(icoVerts[b], coreRot);
        pb = rotateX(pb, coreRot * 0.7);
        pb = rotateY(pb, rotY.current);
        pb = rotateX(pb, rotX.current);

        const projA = project(pa, cx, cy, fov);
        const projB = project(pb, cx, cy, fov);

        ctx.beginPath();
        ctx.moveTo(projA.x, projA.y);
        ctx.lineTo(projB.x, projB.y);
        ctx.stroke();
      }

      // Core inner glow
      const coreGlow = ctx.createRadialGradient(cx, cy, 0, cx, cy, cR * 1.5);
      coreGlow.addColorStop(0, `rgba(0, 180, 255, ${0.08 + Math.sin(time.current * 2) * 0.03})`);
      coreGlow.addColorStop(0.5, "rgba(0, 180, 255, 0.02)");
      coreGlow.addColorStop(1, "rgba(0, 180, 255, 0)");
      ctx.beginPath();
      ctx.arc(cx, cy, cR * 1.5, 0, Math.PI * 2);
      ctx.fillStyle = coreGlow;
      ctx.fill();

      // === DRAW PARTICLES ===
      const pts = particles.current;

      // Slow orbital motion
      for (const p of pts) {
        // Rotate around Y slowly
        const orbitSpeed = 0.002;
        const cos = Math.cos(orbitSpeed);
        const sin = Math.sin(orbitSpeed);
        const nx = p.x * cos - p.z * sin;
        const nz = p.x * sin + p.z * cos;
        p.x = nx + p.vx;
        p.z = nz + p.vz;
        p.y += p.vy + Math.sin(time.current + p.x * 0.01) * 0.1;

        // Keep in sphere
        const dist = Math.sqrt(p.x*p.x + p.y*p.y + p.z*p.z);
        if (dist > FIELD_RADIUS) {
          const s = FIELD_RADIUS / dist * 0.99;
          p.x *= s; p.y *= s; p.z *= s;
        }
        if (dist < CORE_RADIUS + 20) {
          const s = (CORE_RADIUS + 20) / dist * 1.01;
          p.x *= s; p.y *= s; p.z *= s;
        }
      }

      // Sort by z for proper rendering (back to front)
      const transformed: { p: Particle; proj: { x: number; y: number; scale: number }; tz: number }[] = pts.map(p => {
        let tp = rotateY(p, rotY.current);
        tp = rotateX(tp, rotX.current);
        return { p, proj: project(tp, cx, cy, fov), tz: tp.z };
      });
      transformed.sort((a, b) => b.tz - a.tz);

      // Draw connections first (behind particles)
      ctx.lineWidth = 0.4;
      for (let i = 0; i < transformed.length; i++) {
        for (let j = i + 1; j < transformed.length; j++) {
          const dx = transformed[i].proj.x - transformed[j].proj.x;
          const dy = transformed[i].proj.y - transformed[j].proj.y;
          const screenDist = Math.sqrt(dx*dx + dy*dy);
          if (screenDist < CONNECTION_DIST * transformed[i].proj.scale) {
            const alpha = (1 - screenDist / (CONNECTION_DIST * transformed[i].proj.scale)) * 0.12;
            ctx.beginPath();
            ctx.moveTo(transformed[i].proj.x, transformed[i].proj.y);
            ctx.lineTo(transformed[j].proj.x, transformed[j].proj.y);
            ctx.strokeStyle = `rgba(0, 180, 255, ${alpha})`;
            ctx.stroke();
          }
        }
      }

      // Draw particles
      for (const { p, proj } of transformed) {
        const r = p.radius * proj.scale * 1.5;
        if (r < 0.3) continue;

        ctx.beginPath();
        ctx.arc(proj.x, proj.y, r, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(0, 200, 255, ${p.opacity * proj.scale})`;
        ctx.fill();

        // Subtle glow on larger particles
        if (r > 1.5) {
          const pGlow = ctx.createRadialGradient(proj.x, proj.y, 0, proj.x, proj.y, r * 3);
          pGlow.addColorStop(0, `rgba(0, 180, 255, ${0.08 * proj.scale})`);
          pGlow.addColorStop(1, "rgba(0, 180, 255, 0)");
          ctx.beginPath();
          ctx.arc(proj.x, proj.y, r * 3, 0, Math.PI * 2);
          ctx.fillStyle = pGlow;
          ctx.fill();
        }
      }

      // === ENERGY PULSE WAVES from core ===
      const pulseCount = 2;
      for (let i = 0; i < pulseCount; i++) {
        const pulsePhase = (time.current * 0.4 + i * 0.5) % 1;
        const pulseRadius = CORE_RADIUS + pulsePhase * (FIELD_RADIUS - CORE_RADIUS);
        const pulseAlpha = (1 - pulsePhase) * 0.06;
        if (pulseAlpha > 0.005) {
          ctx.beginPath();
          ctx.arc(cx, cy, pulseRadius * project({ x: 0, y: 0, z: 0 }, cx, cy, fov).scale, 0, Math.PI * 2);
          ctx.strokeStyle = `rgba(0, 180, 255, ${pulseAlpha})`;
          ctx.lineWidth = 1.5;
          ctx.stroke();
        }
      }

      // Scan line
      const scanY = ((time.current * 60) % h);
      const scanGrad = ctx.createLinearGradient(0, scanY - 20, 0, scanY + 20);
      scanGrad.addColorStop(0, "rgba(0, 180, 255, 0)");
      scanGrad.addColorStop(0.5, "rgba(0, 180, 255, 0.02)");
      scanGrad.addColorStop(1, "rgba(0, 180, 255, 0)");
      ctx.fillStyle = scanGrad;
      ctx.fillRect(0, scanY - 20, w, 40);

      animRef.current = requestAnimationFrame(animate);
    }

    animRef.current = requestAnimationFrame(animate);

    return () => {
      cancelAnimationFrame(animRef.current);
      window.removeEventListener("resize", resize);
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
      canvas.removeEventListener("mousedown", onMouseDown);
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
        zIndex: 1,
        cursor: "grab",
      }}
    />
  );
}
