import { Canvas } from "@react-three/fiber";
import { OrbitControls } from "@react-three/drei";
import AtomField from "./AtomField";
import OrbitalRings from "./OrbitalRings";
import CoreSphere from "./CoreSphere";

export default function JarvisScene() {
  return (
    <div style={styles.container}>
      <Canvas
        camera={{ position: [0, 0, 10], fov: 60 }}
        style={{ background: "transparent" }}
        gl={{ alpha: true, antialias: true }}
      >
        <ambientLight intensity={0.1} />

        {/* Central core */}
        <CoreSphere />

        {/* Orbital rings around core */}
        <OrbitalRings />

        {/* Particle field */}
        <AtomField />

        {/* Allow subtle mouse interaction */}
        <OrbitControls
          enableZoom={false}
          enablePan={false}
          autoRotate
          autoRotateSpeed={0.3}
          maxPolarAngle={Math.PI * 0.75}
          minPolarAngle={Math.PI * 0.25}
        />
      </Canvas>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: {
    position: "fixed",
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    zIndex: 0,
    pointerEvents: "none",
  },
};
