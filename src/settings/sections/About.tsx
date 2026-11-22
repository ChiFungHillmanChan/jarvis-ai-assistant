import RegistryAbout from '../../components/RegistryAbout';
import { useAiModels } from '../../lib/ai-models';

/** Settings > About: the app version and the model list in use, unchanged from the old About page. */
export default function About() {
  const catalog = useAiModels();
  // RegistryAbout has its own "About JARVIS" heading; the id lets search land here.
  return <div id="about-version"><RegistryAbout catalog={catalog} /></div>;
}
