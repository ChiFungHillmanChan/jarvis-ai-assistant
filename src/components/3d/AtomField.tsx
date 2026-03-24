import { useRef, useMemo, useEffect } from "react";
import { useFrame } from "@react-three/fiber";
import * as THREE from "three";

const PARTICLE_COUNT = 800;
const FIELD_RADIUS = 12;

export default function AtomField() {
  const pointsRef = useRef<THREE.Points>(null);
  const linesRef = useRef<THREE.LineSegments>(null);

  const { positions, velocities, colors } = useMemo(() => {
    const pos = new Float32Array(PARTICLE_COUNT * 3);
    const vel = new Float32Array(PARTICLE_COUNT * 3);
    const col = new Float32Array(PARTICLE_COUNT * 3);

    for (let i = 0; i < PARTICLE_COUNT; i++) {
      const i3 = i * 3;
      const theta = Math.random() * Math.PI * 2;
      const phi = Math.acos(2 * Math.random() - 1);
      const r = FIELD_RADIUS * Math.pow(Math.random(), 0.5);

      pos[i3] = r * Math.sin(phi) * Math.cos(theta);
      pos[i3 + 1] = r * Math.sin(phi) * Math.sin(theta);
      pos[i3 + 2] = r * Math.cos(phi);

      vel[i3] = (Math.random() - 0.5) * 0.005;
      vel[i3 + 1] = (Math.random() - 0.5) * 0.005;
      vel[i3 + 2] = (Math.random() - 0.5) * 0.005;

      col[i3] = 0.0 + Math.random() * 0.1;
      col[i3 + 1] = 0.6 + Math.random() * 0.2;
      col[i3 + 2] = 0.9 + Math.random() * 0.1;
    }
    return { positions: pos, velocities: vel, colors: col };
  }, []);

  const linePositions = useMemo(() => new Float32Array(PARTICLE_COUNT * 6), []);
  const lineColors = useMemo(() => new Float32Array(PARTICLE_COUNT * 6), []);

  // Set up geometries imperatively
  useEffect(() => {
    if (pointsRef.current) {
      const geo = pointsRef.current.geometry;
      geo.setAttribute("position", new THREE.BufferAttribute(positions, 3));
      geo.setAttribute("color", new THREE.BufferAttribute(colors, 3));
    }
    if (linesRef.current) {
      const geo = linesRef.current.geometry;
      geo.setAttribute("position", new THREE.BufferAttribute(linePositions, 3));
      geo.setAttribute("color", new THREE.BufferAttribute(lineColors, 3));
    }
  }, [positions, colors, linePositions, lineColors]);

  useFrame((state) => {
    if (!pointsRef.current) return;
    const posAttr = pointsRef.current.geometry.attributes.position;
    if (!posAttr) return;
    const posArray = posAttr.array as Float32Array;
    const time = state.clock.elapsedTime;

    for (let i = 0; i < PARTICLE_COUNT; i++) {
      const i3 = i * 3;
      const x = posArray[i3];
      const z = posArray[i3 + 2];

      const angle = 0.001 + velocities[i3] * 0.5;
      const cosA = Math.cos(angle);
      const sinA = Math.sin(angle);

      posArray[i3] = x * cosA - z * sinA;
      posArray[i3 + 2] = x * sinA + z * cosA;
      posArray[i3 + 1] += Math.sin(time * 0.3 + i * 0.1) * 0.002;

      const dist = Math.sqrt(posArray[i3] ** 2 + posArray[i3 + 1] ** 2 + posArray[i3 + 2] ** 2);
      if (dist > FIELD_RADIUS) {
        const scale = FIELD_RADIUS / dist;
        posArray[i3] *= scale;
        posArray[i3 + 1] *= scale;
        posArray[i3 + 2] *= scale;
      }
    }
    posAttr.needsUpdate = true;

    if (linesRef.current) {
      const linePosAttr = linesRef.current.geometry.attributes.position;
      const lineColAttr = linesRef.current.geometry.attributes.color;
      if (!linePosAttr || !lineColAttr) return;

      let lineIdx = 0;
      const maxConnections = 150;
      const connectionDist = 2.5;

      for (let i = 0; i < PARTICLE_COUNT && lineIdx < maxConnections; i += 3) {
        const i3 = i * 3;
        for (let j = i + 1; j < PARTICLE_COUNT && lineIdx < maxConnections; j += 3) {
          const j3 = j * 3;
          const dx = posArray[i3] - posArray[j3];
          const dy = posArray[i3 + 1] - posArray[j3 + 1];
          const dz = posArray[i3 + 2] - posArray[j3 + 2];
          const dist = Math.sqrt(dx * dx + dy * dy + dz * dz);

          if (dist < connectionDist) {
            const li = lineIdx * 6;
            linePositions[li] = posArray[i3];
            linePositions[li + 1] = posArray[i3 + 1];
            linePositions[li + 2] = posArray[i3 + 2];
            linePositions[li + 3] = posArray[j3];
            linePositions[li + 4] = posArray[j3 + 1];
            linePositions[li + 5] = posArray[j3 + 2];

            const alpha = 1 - dist / connectionDist;
            lineColors[li] = 0;
            lineColors[li + 1] = 0.7 * alpha;
            lineColors[li + 2] = 1.0 * alpha;
            lineColors[li + 3] = 0;
            lineColors[li + 4] = 0.7 * alpha;
            lineColors[li + 5] = 1.0 * alpha;
            lineIdx++;
          }
        }
      }

      for (let i = lineIdx * 6; i < linePositions.length; i++) {
        linePositions[i] = 0;
        lineColors[i] = 0;
      }

      linePosAttr.needsUpdate = true;
      lineColAttr.needsUpdate = true;
    }
  });

  return (
    <>
      <points ref={pointsRef}>
        <bufferGeometry />
        <pointsMaterial
          size={0.04}
          vertexColors
          transparent
          opacity={0.7}
          blending={THREE.AdditiveBlending}
          depthWrite={false}
        />
      </points>

      <lineSegments ref={linesRef}>
        <bufferGeometry />
        <lineBasicMaterial
          vertexColors
          transparent
          opacity={0.3}
          blending={THREE.AdditiveBlending}
          depthWrite={false}
        />
      </lineSegments>
    </>
  );
}
