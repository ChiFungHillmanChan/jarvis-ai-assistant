import { useRef } from "react";
import { useFrame } from "@react-three/fiber";
import * as THREE from "three";

export default function CoreSphere() {
  const meshRef = useRef<THREE.Mesh>(null);
  const glowRef = useRef<THREE.Mesh>(null);

  useFrame((state) => {
    const t = state.clock.elapsedTime;
    if (meshRef.current) {
      meshRef.current.rotation.y += 0.002;
      meshRef.current.rotation.x = Math.sin(t * 0.3) * 0.1;
      // Pulse effect
      const scale = 1 + Math.sin(t * 1.5) * 0.03;
      meshRef.current.scale.setScalar(scale);
    }
    if (glowRef.current) {
      const glowScale = 1.5 + Math.sin(t * 1.5) * 0.1;
      glowRef.current.scale.setScalar(glowScale);
      (glowRef.current.material as THREE.MeshBasicMaterial).opacity = 0.04 + Math.sin(t * 1.5) * 0.02;
    }
  });

  return (
    <>
      {/* Core sphere -- wireframe */}
      <mesh ref={meshRef}>
        <icosahedronGeometry args={[0.8, 1]} />
        <meshBasicMaterial
          color="#00b4ff"
          wireframe
          transparent
          opacity={0.3}
        />
      </mesh>

      {/* Inner solid core */}
      <mesh>
        <sphereGeometry args={[0.3, 16, 16]} />
        <meshBasicMaterial
          color="#00b4ff"
          transparent
          opacity={0.15}
        />
      </mesh>

      {/* Glow sphere */}
      <mesh ref={glowRef}>
        <sphereGeometry args={[1.2, 16, 16]} />
        <meshBasicMaterial
          color="#00b4ff"
          transparent
          opacity={0.04}
          side={THREE.BackSide}
        />
      </mesh>
    </>
  );
}
