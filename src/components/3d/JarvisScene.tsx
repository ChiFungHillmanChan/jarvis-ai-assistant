import { useEffect, useRef } from "react";

/**
 * JARVIS Holographic Sphere -- inspired by the Iron Man movie.
 * Massive glowing sphere with latitude/longitude grid lines,
 * orbiting rings, energy particles, bright pulsing core,
 * and tons of flowing detail lines.
 */

interface Point3D { x: number; y: number; z: number; }

const SPHERE_RADIUS = 280;
const CORE_RADIUS = 45;
const PARTICLE_COUNT = 350;
const LAT_LINES = 12;
const LON_LINES = 16;
const DETAIL_LINES = 60;

function project(p: Point3D, cx: number, cy: number, fov: number) {
  const z = p.z + 700;
  const scale = fov / (fov + z);
  return { x: cx + p.x * scale, y: cy + p.y * scale, scale: Math.max(scale, 0.1) };
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

export default function JarvisScene() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const animRef = useRef(0);
  const time = useRef(0);
  const rotYRef = useRef(0);
  const rotXRef = useRef(0.25);
  const targetRY = useRef(0);
  const targetRX = useRef(0.25);
  const dragging = useRef(false);
  const lastM = useRef({ x: 0, y: 0 });
  const autoRot = useRef(true);

  // Particles stored as spherical coords for stable orbiting
  const particles = useRef<{ theta: number; phi: number; r: number; speed: number; size: number; bright: number }[]>([]);

  // Random detail lines (arcs on the sphere surface)
  const detailArcs = useRef<{ theta1: number; phi1: number; theta2: number; phi2: number; speed: number }[]>([]);

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

    // Mouse controls -- listen on window so it works even when UI layer is on top
    function onDown(e: MouseEvent) {
      // Only start drag if clicking on non-interactive area (not buttons, inputs, panels, sidebar)
      const target = e.target as HTMLElement;
      if (target.closest("button, input, select, textarea, .panel, .no-drag, nav, form")) return;
      dragging.current = true;
      lastM.current = { x: e.clientX, y: e.clientY };
      autoRot.current = false;
    }
    function onMove(e: MouseEvent) {
      if (!dragging.current) return;
      targetRY.current += (e.clientX - lastM.current.x) * 0.005;
      targetRX.current += (e.clientY - lastM.current.y) * 0.005;
      targetRX.current = Math.max(-1.5, Math.min(1.5, targetRX.current));
      lastM.current = { x: e.clientX, y: e.clientY };
    }
    function onUp() { dragging.current = false; setTimeout(() => { if (!dragging.current) autoRot.current = true; }, 2000); }
    window.addEventListener("mousedown", onDown);
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);

    // Init particles on sphere surface + some inside
    particles.current = [];
    for (let i = 0; i < PARTICLE_COUNT; i++) {
      const onSurface = Math.random() > 0.3;
      particles.current.push({
        theta: Math.random() * Math.PI * 2,
        phi: Math.acos(2 * Math.random() - 1),
        r: onSurface ? SPHERE_RADIUS * (0.85 + Math.random() * 0.2) : CORE_RADIUS + Math.random() * (SPHERE_RADIUS - CORE_RADIUS),
        speed: (0.001 + Math.random() * 0.004) * (Math.random() > 0.5 ? 1 : -1),
        size: 0.5 + Math.random() * 2,
        bright: 0.3 + Math.random() * 0.7,
      });
    }

    // Init detail arcs (random arcs along sphere surface)
    detailArcs.current = [];
    for (let i = 0; i < DETAIL_LINES; i++) {
      const t1 = Math.random() * Math.PI * 2;
      const p1 = Math.random() * Math.PI;
      const spread = 0.3 + Math.random() * 0.8;
      detailArcs.current.push({
        theta1: t1, phi1: p1,
        theta2: t1 + (Math.random() - 0.5) * spread,
        phi2: p1 + (Math.random() - 0.5) * spread,
        speed: (Math.random() - 0.5) * 0.003,
      });
    }

    function sphereToCart(theta: number, phi: number, r: number): Point3D {
      return {
        x: r * Math.sin(phi) * Math.cos(theta),
        y: r * Math.cos(phi),
        z: r * Math.sin(phi) * Math.sin(theta),
      };
    }

    function animate() {
      if (!canvas || !ctx) return;
      time.current += 0.006;
      const w = canvas.width, h = canvas.height;
      const cx = w / 2, cy = h / 2;
      const fov = 500;

      if (autoRot.current) targetRY.current += 0.002;
      rotYRef.current += (targetRY.current - rotYRef.current) * 0.06;
      rotXRef.current += (targetRX.current - rotXRef.current) * 0.06;
      const ry = rotYRef.current, rx = rotXRef.current;

      // === BACKGROUND ===
      ctx.fillStyle = "#060a14";
      ctx.fillRect(0, 0, w, h);

      // Ambient glow
      const ambGlow = ctx.createRadialGradient(cx, cy, 0, cx, cy, SPHERE_RADIUS * 1.8);
      ambGlow.addColorStop(0, "rgba(0, 60, 120, 0.15)");
      ambGlow.addColorStop(0.3, "rgba(0, 30, 60, 0.08)");
      ambGlow.addColorStop(0.6, "rgba(0, 15, 30, 0.03)");
      ambGlow.addColorStop(1, "rgba(0, 0, 0, 0)");
      ctx.fillStyle = ambGlow;
      ctx.fillRect(0, 0, w, h);

      // === LATITUDE LINES (horizontal rings) ===
      for (let i = 1; i < LAT_LINES; i++) {
        const phi = (i / LAT_LINES) * Math.PI;
        const ringR = SPHERE_RADIUS * Math.sin(phi);
        const ringY = SPHERE_RADIUS * Math.cos(phi);
        const segs = 60;

        ctx.beginPath();
        for (let s = 0; s <= segs; s++) {
          const theta = (s / segs) * Math.PI * 2;
          let p: Point3D = { x: ringR * Math.cos(theta), y: ringY, z: ringR * Math.sin(theta) };
          p = transform(p, ry, rx);
          const pr = project(p, cx, cy, fov);
          if (s === 0) ctx.moveTo(pr.x, pr.y);
          else ctx.lineTo(pr.x, pr.y);
        }
        ctx.strokeStyle = `rgba(0, 160, 255, ${0.06 + (i === LAT_LINES / 2 ? 0.06 : 0)})`;
        ctx.lineWidth = i === LAT_LINES / 2 ? 1.2 : 0.6;
        ctx.stroke();
      }

      // === LONGITUDE LINES (vertical meridians) ===
      for (let i = 0; i < LON_LINES; i++) {
        const theta = (i / LON_LINES) * Math.PI * 2;
        const segs = 40;

        ctx.beginPath();
        for (let s = 0; s <= segs; s++) {
          const phi = (s / segs) * Math.PI;
          let p = sphereToCart(theta, phi, SPHERE_RADIUS);
          p = transform(p, ry, rx);
          const pr = project(p, cx, cy, fov);
          if (s === 0) ctx.moveTo(pr.x, pr.y);
          else ctx.lineTo(pr.x, pr.y);
        }
        ctx.strokeStyle = "rgba(0, 160, 255, 0.05)";
        ctx.lineWidth = 0.5;
        ctx.stroke();
      }

      // === DETAIL ARC LINES (flowing energy on surface) ===
      for (const arc of detailArcs.current) {
        arc.theta1 += arc.speed;
        arc.theta2 += arc.speed;

        const steps = 15;
        ctx.beginPath();
        for (let s = 0; s <= steps; s++) {
          const t = s / steps;
          const theta = arc.theta1 + (arc.theta2 - arc.theta1) * t;
          const phi = arc.phi1 + (arc.phi2 - arc.phi1) * t;
          let p = sphereToCart(theta, phi, SPHERE_RADIUS * 0.98);
          p = transform(p, ry, rx);
          const pr = project(p, cx, cy, fov);
          if (s === 0) ctx.moveTo(pr.x, pr.y);
          else ctx.lineTo(pr.x, pr.y);
        }
        ctx.strokeStyle = `rgba(0, 200, 255, ${0.08 + Math.sin(time.current * 2 + arc.theta1) * 0.04})`;
        ctx.lineWidth = 0.4;
        ctx.stroke();
      }

      // === ORBITAL RINGS (tilted, with energy dots) ===
      const rings = [
        { r: SPHERE_RADIUS * 1.1, tiltX: 0.5, tiltZ: 0.2, speed: 0.4, opacity: 0.12, width: 1.2 },
        { r: SPHERE_RADIUS * 0.75, tiltX: 1.2, tiltZ: 0.6, speed: -0.3, opacity: 0.1, width: 1 },
        { r: SPHERE_RADIUS * 1.25, tiltX: 0.1, tiltZ: 1.0, speed: 0.25, opacity: 0.08, width: 0.8 },
        { r: SPHERE_RADIUS * 0.55, tiltX: 0.8, tiltZ: 0.3, speed: 0.55, opacity: 0.1, width: 1 },
        { r: SPHERE_RADIUS * 1.4, tiltX: 0.3, tiltZ: 0.7, speed: -0.2, opacity: 0.06, width: 0.6 },
        { r: SPHERE_RADIUS * 0.9, tiltX: 1.5, tiltZ: 0.1, speed: 0.35, opacity: 0.09, width: 0.9 },
        { r: SPHERE_RADIUS * 1.05, tiltX: 0.7, tiltZ: 1.3, speed: -0.45, opacity: 0.07, width: 0.7 },
      ];

      for (const ring of rings) {
        const segs = 80;
        ctx.beginPath();
        for (let s = 0; s <= segs; s++) {
          const a = (s / segs) * Math.PI * 2;
          let p: Point3D = { x: Math.cos(a) * ring.r, y: Math.sin(a) * ring.r, z: 0 };
          p = rotateX(p, ring.tiltX);
          p = rotateZ(p, ring.tiltZ);
          p = transform(p, ry, rx);
          const pr = project(p, cx, cy, fov);
          if (s === 0) ctx.moveTo(pr.x, pr.y);
          else ctx.lineTo(pr.x, pr.y);
        }
        ctx.strokeStyle = `rgba(0, 180, 255, ${ring.opacity})`;
        ctx.lineWidth = ring.width;
        ctx.stroke();

        // Energy dot
        const da = time.current * ring.speed;
        let dp: Point3D = { x: Math.cos(da) * ring.r, y: Math.sin(da) * ring.r, z: 0 };
        dp = rotateX(dp, ring.tiltX);
        dp = rotateZ(dp, ring.tiltZ);
        dp = transform(dp, ry, rx);
        const dpr = project(dp, cx, cy, fov);

        const gs = 18 * dpr.scale;
        const dg = ctx.createRadialGradient(dpr.x, dpr.y, 0, dpr.x, dpr.y, gs);
        dg.addColorStop(0, "rgba(100, 220, 255, 0.6)");
        dg.addColorStop(0.3, "rgba(0, 180, 255, 0.2)");
        dg.addColorStop(1, "rgba(0, 180, 255, 0)");
        ctx.beginPath(); ctx.arc(dpr.x, dpr.y, gs, 0, Math.PI * 2); ctx.fillStyle = dg; ctx.fill();
        ctx.beginPath(); ctx.arc(dpr.x, dpr.y, 2.5 * dpr.scale, 0, Math.PI * 2);
        ctx.fillStyle = "rgba(180, 240, 255, 0.95)"; ctx.fill();
      }

      // === CORE ===
      const breath = 1 + Math.sin(time.current * 2.5) * 0.08;
      const cR = CORE_RADIUS * breath;

      // Bright core glow (multiple layers)
      for (let layer = 0; layer < 3; layer++) {
        const glowR = cR * (3 - layer);
        const glowAlpha = [0.12, 0.06, 0.03][layer];
        const cg = ctx.createRadialGradient(cx, cy, 0, cx, cy, glowR);
        cg.addColorStop(0, `rgba(80, 200, 255, ${glowAlpha + Math.sin(time.current * 2.5) * 0.02})`);
        cg.addColorStop(0.5, `rgba(0, 120, 255, ${glowAlpha * 0.3})`);
        cg.addColorStop(1, "rgba(0, 60, 120, 0)");
        ctx.beginPath(); ctx.arc(cx, cy, glowR, 0, Math.PI * 2); ctx.fillStyle = cg; ctx.fill();
      }

      // Core wireframe (double icosahedron)
      const t2 = (1 + Math.sqrt(5)) / 2;
      const rawVerts: Point3D[] = [
        {x:-1,y:t2,z:0},{x:1,y:t2,z:0},{x:-1,y:-t2,z:0},{x:1,y:-t2,z:0},
        {x:0,y:-1,z:t2},{x:0,y:1,z:t2},{x:0,y:-1,z:-t2},{x:0,y:1,z:-t2},
        {x:t2,y:0,z:-1},{x:t2,y:0,z:1},{x:-t2,y:0,z:-1},{x:-t2,y:0,z:1},
      ];
      const edges = [
        [0,1],[0,5],[0,7],[0,10],[0,11],[1,5],[1,7],[1,8],[1,9],
        [2,3],[2,4],[2,6],[2,10],[2,11],[3,4],[3,6],[3,8],[3,9],
        [4,5],[4,9],[4,11],[5,9],[5,11],[6,7],[6,8],[6,10],[7,8],[7,10],[8,9],[10,11],
      ];

      for (let layer = 0; layer < 2; layer++) {
        const scale = layer === 0 ? cR : cR * 0.6;
        const rot = time.current * (0.4 + layer * 0.2) * (layer === 0 ? 1 : -1);
        const alpha = layer === 0 ? 0.35 : 0.2;

        const verts = rawVerts.map(v => {
          const len = Math.sqrt(v.x*v.x + v.y*v.y + v.z*v.z);
          return { x: v.x/len * scale, y: v.y/len * scale, z: v.z/len * scale };
        });

        ctx.strokeStyle = `rgba(100, 220, 255, ${alpha})`;
        ctx.lineWidth = layer === 0 ? 1 : 0.6;
        for (const [a, b] of edges) {
          let pa = rotateY(verts[a], rot); pa = rotateX(pa, rot * 0.7);
          pa = transform(pa, ry, rx);
          let pb = rotateY(verts[b], rot); pb = rotateX(pb, rot * 0.7);
          pb = transform(pb, ry, rx);
          const pra = project(pa, cx, cy, fov);
          const prb = project(pb, cx, cy, fov);
          ctx.beginPath(); ctx.moveTo(pra.x, pra.y); ctx.lineTo(prb.x, prb.y); ctx.stroke();
        }
      }

      // Core center bright dot
      const centerGlow = ctx.createRadialGradient(cx, cy, 0, cx, cy, cR * 0.4);
      centerGlow.addColorStop(0, `rgba(200, 240, 255, ${0.3 + Math.sin(time.current * 3) * 0.1})`);
      centerGlow.addColorStop(1, "rgba(0, 120, 255, 0)");
      ctx.beginPath(); ctx.arc(cx, cy, cR * 0.4, 0, Math.PI * 2); ctx.fillStyle = centerGlow; ctx.fill();

      // === PARTICLES ===
      const pts = particles.current;
      for (const p of pts) {
        p.theta += p.speed;

        let cart = sphereToCart(p.theta, p.phi, p.r);
        cart = transform(cart, ry, rx);
        const pr = project(cart, cx, cy, fov);
        const sz = p.size * pr.scale * 1.5;
        if (sz < 0.2) continue;

        ctx.beginPath();
        ctx.arc(pr.x, pr.y, sz, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(100, 220, 255, ${p.bright * pr.scale * 0.7})`;
        ctx.fill();

        if (sz > 1.2) {
          const pg = ctx.createRadialGradient(pr.x, pr.y, 0, pr.x, pr.y, sz * 4);
          pg.addColorStop(0, `rgba(0, 180, 255, ${0.06 * pr.scale})`);
          pg.addColorStop(1, "rgba(0, 0, 0, 0)");
          ctx.beginPath(); ctx.arc(pr.x, pr.y, sz * 4, 0, Math.PI * 2); ctx.fillStyle = pg; ctx.fill();
        }
      }

      // === ENERGY PULSE WAVES ===
      for (let i = 0; i < 3; i++) {
        const phase = (time.current * 0.3 + i * 0.33) % 1;
        const pr = CORE_RADIUS + phase * (SPHERE_RADIUS * 1.1 - CORE_RADIUS);
        const alpha = (1 - phase) * 0.08;
        if (alpha < 0.005) continue;
        const projScale = project({ x: 0, y: 0, z: 0 }, cx, cy, fov).scale;
        ctx.beginPath();
        ctx.arc(cx, cy, pr * projScale, 0, Math.PI * 2);
        ctx.strokeStyle = `rgba(0, 200, 255, ${alpha})`;
        ctx.lineWidth = 2 * (1 - phase);
        ctx.stroke();
      }

      // === OUTER SPHERE BOUNDARY (faint) ===
      const outerScale = project({ x: 0, y: 0, z: 0 }, cx, cy, fov).scale;
      ctx.beginPath();
      ctx.arc(cx, cy, SPHERE_RADIUS * outerScale, 0, Math.PI * 2);
      ctx.strokeStyle = "rgba(0, 160, 255, 0.04)";
      ctx.lineWidth = 1;
      ctx.stroke();

      animRef.current = requestAnimationFrame(animate);
    }

    animRef.current = requestAnimationFrame(animate);
    return () => {
      cancelAnimationFrame(animRef.current);
      window.removeEventListener("resize", resize);
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, []);

  return (
    <canvas ref={canvasRef} style={{
      position: "fixed", top: 0, left: 0, width: "100%", height: "100%",
      zIndex: 1, pointerEvents: "none",
    }} />
  );
}
