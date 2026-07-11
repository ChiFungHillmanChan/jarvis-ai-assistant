// Central icon module. Everything in the app chrome imports icons from here so
// stroke weight and brand marks stay consistent. lucide dropped brand logos, so
// GitHub and Notion are hand-drawn to match lucide's 24x24 / stroke-1.75 grid.
export {
  LayoutDashboard,
  MessageSquare,
  Mail,
  Calendar,
  Clock,
  Rocket,
  Settings,
  Plus,
  X,
  Maximize2,
  Sparkles,
  ArrowUp,
  Minimize2,
  Sliders,
  Cpu,
  KeyRound as Key,
  Globe,
  Gem,
  Mic,
  Bot,
  Palette,
  CreditCard,
} from "lucide-react";

interface MarkProps { size?: number }

export function GithubMark({ size = 20 }: MarkProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none"
      stroke="currentColor" strokeWidth={1.75} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M9 19c-5 1.5-5-2.5-7-3m14 6v-3.87a3.37 3.37 0 0 0-.94-2.61c3.14-.35 6.44-1.54 6.44-7A5.44 5.44 0 0 0 20 4.77 5.07 5.07 0 0 0 19.91 1S18.73.65 16 2.48a13.38 13.38 0 0 0-7 0C6.27.65 5.09 1 5.09 1A5.07 5.07 0 0 0 5 4.77a5.44 5.44 0 0 0-1.5 3.78c0 5.42 3.3 6.61 6.44 7A3.37 3.37 0 0 0 9 18.13V22" />
    </svg>
  );
}

export function NotionMark({ size = 20 }: MarkProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none"
      stroke="currentColor" strokeWidth={1.75} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="3" y="3" width="18" height="18" rx="2.5" />
      <path d="M8.5 8.2v7.6" />
      <path d="M8.5 8.2l7 7.6" />
      <path d="M15.5 8.2v7.6" />
    </svg>
  );
}
