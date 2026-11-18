"use client";

/** Read-only schema graph: one database hub with its collections around it. */

function short(value, max = 18) {
  const text = String(value || "");
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

export default function DatabaseGraph({ databaseName, collections, basePath }) {
  if (!collections || collections.length === 0) {
    return <div className="graph-empty small subtle">No collections to graph yet.</div>;
  }

  const width = 780;
  const perColumn = Math.ceil(collections.length / 2);
  const height = Math.max(260, perColumn * 96 + 60);
  const cy = height / 2;
  const positions = collections.map((collection, index) => ({
    x: index < perColumn ? 430 : 650,
    y: 56 + (index % perColumn) * 96,
  }));

  return (
    <div className="graph">
      <svg
        className="graph-svg"
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label={`Collections in ${databaseName}`}
      >
        {collections.map((collection, index) => {
          const point = positions[index];
          return (
            <path
              key={`link-${collection.$id}`}
              className="graph-link"
              d={`M 178 ${cy} Q 300 ${(cy + point.y) / 2} ${point.x - 20} ${point.y}`}
              fill="none"
              stroke="var(--line)"
              strokeWidth="1.5"
              strokeDasharray="4 4"
            />
          );
        })}

        <g className="graph-node" aria-label={`Database ${databaseName}`}>
          <circle
            cx="150"
            cy={cy}
            r="26"
            fill="var(--panel-raised)"
            stroke="var(--accent)"
            strokeWidth="2"
          />
          <text className="graph-initial" x="150" y={cy + 4} textAnchor="middle" fill="var(--accent)">
            DB
          </text>
          <text className="graph-name" x="150" y={cy + 46} textAnchor="middle">
            {short(databaseName, 20)}
          </text>
        </g>

        {collections.map((collection, index) => {
          const point = positions[index];
          const color = collection.documentSecurity ? "var(--ok)" : "var(--muted)";
          return (
            <a
              key={collection.$id}
              className="graph-node"
              href={`${basePath}/${collection.$id}`}
              aria-label={`${collection.name}, ${collection.attributes.length} attributes`}
            >
              <circle cx={point.x} cy={point.y} r="16" fill="var(--panel-raised)" stroke={color} strokeWidth="2" />
              <text className="graph-initial" x={point.x} y={point.y + 4} textAnchor="middle" fill={color}>
                {collection.name.slice(0, 2).toUpperCase()}
              </text>
              <text className="graph-name" x={point.x} y={point.y + 30} textAnchor="middle">
                {short(collection.name)}
              </text>
              <text className="graph-meta" x={point.x} y={point.y + 44} textAnchor="middle">
                {collection.attributes.length} attrs
              </text>
            </a>
          );
        })}
      </svg>
      <div className="graph-hint small subtle">
        Click a collection to open its attributes. Green nodes use document-level security.
      </div>
    </div>
  );
}
