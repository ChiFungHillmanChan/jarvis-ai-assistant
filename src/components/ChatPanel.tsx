import AssistantPanel from '../workspace/AssistantPanel';

// Compatibility entry point for extensions importing the former overlay.
export default function ChatPanel({ isOpen, onClose }: { isOpen: boolean; onClose: () => void; onNavigateToChat: () => void }) {
  return isOpen ? <AssistantPanel onClose={onClose} /> : null;
}
