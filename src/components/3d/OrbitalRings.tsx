import { useRef } from "react";
import { useFrame } from "@react-three/fiber";
import * as THREE from "three";

interface RingProps {
  radius: number;
  speed: number;
  tilt: [number, number, number];
  color: string;
  opacity: number;
}

function Ring({ radius, speed, tilt, color, opacity }: RingProps) {
  const ref = useRef<THREE.Mesh>(null);

  useFrame((state) => {
    if (ref.current) {
      ref.current.rotation.z += speed;
      // Subtle breathing effect
      ref.current.scale.setScalar(1 + Math.sin(state.clock.elapsedTime * 0.5) * 0.02);
    }
  });

  return (
    <mesh ref={ref} rotation={tilt}>
      <torusGeometry args={[radius, 0.008, 16, 100]} />
      <meshBasicMaterial
        color={color}
        transparent
        opacity={opacity}
        side={THREE.DoubleSide}
      />
    </mesh>
  );
}

// Orbiting dot on ring
function OrbiterDot({ radius, speed, tilt, color }: { radius: number; speed: number; tilt: [number, number, number]; color: string }) {
  const ref = useRef<THREE.Mesh>(null);
  const angle = useRef(Math.random() * Math.PI * 2);

  useFrame(() => {
    if (ref.current) {
      angle.current += speed * 2;
      ref.current.position.x = Math.cos(angle.current) * radius;
      ref.current.position.y = Math.sin(angle.current) * radius;
    }
  });

  return (
    <group rotation={tilt}>
      <mesh ref={ref}>
        <sphereGeometry args={[0.06, 8, 8]} />
        <meshBasicMaterial color={color} transparent opacity={0.9} />
      </mesh>
    </group>
  );
}

export default function OrbitalRings() {
  return (
    <>
      {/* Inner ring -- fast */}
      <Ring radius={3} speed={0.003} tilt={[0.3, 0.2, 0]} color="#00b4ff" opacity={0.15} />
      <OrbiterDot radius={3} speed={0.003} tilt={[0.3, 0.2, 0]} color="#00b4ff" />

      {/* Middle ring -- medium */}
      <Ring radius={5} speed={-0.002} tilt={[0.8, 0, 0.4]} color="#00b4ff" opacity={0.1} />
      <OrbiterDot radius={5} speed={-0.002} tilt={[0.8, 0, 0.4]} color="#00d4ff" />

      {/* Outer ring -- slow */}
      <Ring radius={7.5} speed={0.001} tilt={[1.2, 0.5, 0]} color="#0088cc" opacity={0.06} />
      <OrbiterDot radius={7.5} speed={0.001} tilt={[1.2, 0.5, 0]} color="#0088cc" />

      {/* Accent ring */}
      <Ring radius={4} speed={0.0015} tilt={[0.5, 1.0, 0.2]} color="#00ffcc" opacity={0.08} />
    </>
  );
}
