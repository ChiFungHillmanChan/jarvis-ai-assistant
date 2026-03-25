import { memo } from "react";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface StatusCardProps {
  statusType: string;
  data: Record<string, unknown>;
}

interface StatusConfig {
  colorBase: string;
  label: string;
  icon: "check" | "sync";
}

// ---------------------------------------------------------------------------
// Config map
// ---------------------------------------------------------------------------

const STATUS_CONFIG: Record<string, StatusConfig> = {
  task_created: {
    colorBase: "16, 185, 129",
    label: "CREATED",
    icon: "check",
  },
  task_completed: {
    colorBase: "16, 185, 129",
    label: "COMPLETED",
    icon: "check",
  },
  email_synced: {
    colorBase: "0, 180, 255",
    label: "SYNCED",
    icon: "sync",
  },
  calendar_synced: {
    colorBase: "0, 180, 255",
    label: "SYNCED",
    icon: "sync",
  },
  cron_created: {
    colorBase: "16, 185, 129",
    label: "SCHEDULED",
    icon: "check",
  },
  action_completed: {
    colorBase: "16, 185, 129",
    label: "DONE",
    icon: "check",
  },
};

// ---------------------------------------------------------------------------
// Icons
// ---------------------------------------------------------------------------

function CheckIcon({ color }: { color: string }) {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
      <polyline
        points="3,7 6,10 11,4"
        stroke={color}
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function SyncIcon({ color }: { color: string }) {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
      <path
        d="M10.5 3.5A4.5 4.5 0 0 0 3 5.5M3.5 10.5A4.5 4.5 0 0 0 11 8.5"
        stroke={color}
        strokeWidth="1.2"
        strokeLinecap="round"
      />
      <polyline
        points="3,3 3,6 6,6"
        stroke={color}
        strokeWidth="1.2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <polyline
        points="11,11 11,8 8,8"
        stroke={color}
        strokeWidth="1.2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

// ---------------------------------------------------------------------------
// Detail extraction
// ---------------------------------------------------------------------------

function extractDetails(data: Record<string, unknown>): string[] {
  const details: string[] = [];

  if (data.priority != null) details.push(`Priority: ${data.priority}`);
  if (data.due) details.push(`Due: ${data.due}`);
  if (data.count != null) details.push(`Count: ${data.count}`);
  if (data.folder) details.push(`Folder: ${data.folder}`);
  if (data.schedule) details.push(`Schedule: ${data.schedule}`);
  if (data.description) details.push(`${data.description}`);
  if (data.result) details.push(`${data.result}`);

  return details;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

function StatusCardInner({ statusType, data }: StatusCardProps) {
  const config = STATUS_CONFIG[statusType] ?? {
    colorBase: "0, 180, 255",
    label: statusType.toUpperCase().replace(/_/g, " "),
    icon: "check" as const,
  };

  const color = `rgba(${config.colorBase}, 0.8)`;
  const colorDim = `rgba(${config.colorBase}, 0.15)`;
  const colorFaint = `rgba(${config.colorBase}, 0.06)`;
  const colorBorder = `rgba(${config.colorBase}, 0.2)`;

  const title = (data.title as string) || (data.name as string) || statusType.replace(/_/g, " ");
  const details = extractDetails(data);

  return (
    <div
      style={{
        ...cardStyles.container,
        background: colorFaint,
        borderColor: colorBorder,
      }}
    >
      {/* Icon box */}
      <div
        style={{
          ...cardStyles.iconBox,
          background: colorDim,
          borderColor: colorBorder,
        }}
      >
        {config.icon === "check" ? (
          <CheckIcon color={color} />
        ) : (
          <SyncIcon color={color} />
        )}
      </div>

      {/* Content */}
      <div style={cardStyles.content}>
        <div style={{ ...cardStyles.title, color }}>{title}</div>
        {details.length > 0 && (
          <div style={cardStyles.details}>
            {details.map((d, i) => (
              <span key={i} style={cardStyles.detail}>
                {d}
              </span>
            ))}
          </div>
        )}
      </div>

      {/* Status badge */}
      <div
        style={{
          ...cardStyles.badge,
          color,
          background: colorDim,
          borderColor: colorBorder,
        }}
      >
        {config.label}
      </div>
    </div>
  );
}

const StatusCard = memo(StatusCardInner);
export default StatusCard;

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

const cardStyles: Record<string, React.CSSProperties> = {
  container: {
    display: "flex",
    alignItems: "center",
    gap: 10,
    border: "1px solid",
    borderRadius: 6,
    padding: "8px 10px",
    marginTop: 6,
  },
  iconBox: {
    width: 28,
    height: 28,
    minWidth: 28,
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 6,
    border: "1px solid",
  },
  content: {
    flex: 1,
    minWidth: 0,
    overflow: "hidden",
  },
  title: {
    fontSize: 12,
    fontWeight: 600,
    fontFamily: "var(--font-sans)",
    whiteSpace: "nowrap",
    overflow: "hidden",
    textOverflow: "ellipsis",
  },
  details: {
    display: "flex",
    flexWrap: "wrap",
    gap: 8,
    marginTop: 2,
  },
  detail: {
    fontSize: 10,
    fontFamily: "var(--font-mono)",
    color: "rgba(0, 180, 255, 0.5)",
  },
  badge: {
    fontSize: 9,
    fontFamily: "var(--font-mono)",
    fontWeight: 600,
    letterSpacing: 1,
    padding: "2px 7px",
    borderRadius: 3,
    border: "1px solid",
    whiteSpace: "nowrap",
  },
};
