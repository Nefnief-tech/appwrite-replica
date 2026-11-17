"use client";

/** Replication topology for the registered Postgres nodes. Read-only SVG. */

function health(node) {
  if (!node.status?.ok) return "down";
  const lag = node.status.lagSeconds;
  if (lag != null && lag > 5) return "lagging";
  return "healthy";
}

function lagLabel(node) {
  const status = node.status;
  if (!status?.ok) return status?.error || "offline";
  if (status.lagSeconds === 0) return "in sync";
  if (status.lagSeconds != null) return `${status.lagSeconds.toFixed(1)}s lag`;
  return status.inRecovery ? "streaming" : "primary";
}

const COLOUR = {
  healthy: "var(--ok)",
  lagging: "var(--warn)",
  down: "var(--danger)",
};

export default function NodeGraph({ nodes, selectedId, onSelect }) {
  if (!nodes || nodes.length === 0) {
    return <div className="graph-empty small subtle">No nodes registered yet.</div>;
  }

  const leader = nodes.find((node) => node.role === "leader") || nodes[0];
  const followers = nodes.filter((node) => node.id !== leader.id);
  const width = 720;
  const height = Math.max(220, followers.length * 92 + 60);
  const cy = height / 2;
  const positions = new Map([[leader.id, { x: 170, y: cy }]]);
  followers.forEach((node, index) => {
    positions.set(node.id, {
      x: 540,
      y: cy + (index - (followers.length - 1) / 2) * 92,
    });
  });

  const selected = nodes.find((node) => node.id === selectedId);

  function handleKey(event, id) {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onSelect(id === selectedId ? null : id);
    }
  }

  return (
    <div className="graph">
      <svg
        className="graph-svg"
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label="Postgres replication topology"
      >
        {followers.map((node) => {
          const to = positions.get(node.id);
          const condition = health(node);
          return (
            <path
              key={`link-${node.id}`}
              className={`graph-link ${condition}`}
              d={`M 197 ${cy} Q 360 ${(cy + to.y) / 2} ${to.x - 15} ${to.y}`}
              fill="none"
              stroke={COLOUR[condition]}
              strokeWidth="1.5"
            />
          );
        })}

        {nodes.map((node) => {
          const point = positions.get(node.id);
          const isLeader = node.id === leader.id;
          const condition = health(node);
          const active = node.id === selectedId;
          return (
            <g
              key={node.id}
              className={`graph-node ${condition} ${active ? "selected" : ""}`}
              role="button"
              tabIndex={0}
              aria-label={`${node.name}, ${node.role}, ${lagLabel(node)}`}
              onClick={() => onSelect(active ? null : node.id)}
              onKeyDown={(event) => handleKey(event, node.id)}
            >
              {isLeader ? (
                <circle
                  className="graph-ring"
                  cx={point.x}
                  cy={point.y}
                  r="28"
                  fill="none"
                  stroke="var(--accent)"
                  strokeWidth="1"
                  strokeDasharray="3 4"
                />
              ) : null}
              <circle
                cx={point.x}
                cy={point.y}
                r={isLeader ? 20 : 13}
                fill="var(--panel-raised)"
                stroke={COLOUR[condition]}
                strokeWidth="2"
              />
              <text
                className="graph-initial"
                x={point.x}
                y={point.y + 4}
                textAnchor="middle"
                fill={COLOUR[condition]}
              >
                {isLeader ? "L" : "F"}
              </text>
              <text className="graph-name" x={point.x} y={point.y + (isLeader ? 44 : 34)} textAnchor="middle">
                {node.name}
              </text>
              <text className="graph-meta" x={point.x} y={point.y + (isLeader ? 58 : 48)} textAnchor="middle">
                {lagLabel(node)}
              </text>
            </g>
          );
        })}
      </svg>

      {selected ? (
        <div className="graph-detail">
          <div><span className="mono">Node</span><strong>{selected.name}</strong></div>
          <div><span className="mono">Role</span><strong>{selected.role}{selected.isWriter ? " · writer" : ""}</strong></div>
          <div><span className="mono">Health</span><strong style={{ color: COLOUR[health(selected)] }}>{lagLabel(selected)}</strong></div>
          <div><span className="mono">Latency</span><strong>{selected.status?.latencyMs != null ? `${selected.status.latencyMs} ms` : "—"}</strong></div>
          <div><span className="mono">Endpoint</span><strong className="mono">{selected.dsnHint}</strong></div>
        </div>
      ) : (
        <div className="graph-hint small subtle">Select a node to see its details.</div>
      )}
    </div>
  );
}
