"use client";

const compact = new Intl.NumberFormat(undefined, { notation: "compact" });

function niceCeil(value) {
  if (value <= 4) return 4;
  const power = 10 ** Math.floor(Math.log10(value));
  for (const step of [1, 2, 5, 10]) {
    if (step * power >= value) return step * power;
  }
  return 10 * power;
}

function guid(axis, index) {
  return `${axis}-${index}`;
}

/**
 * One accent area chart. Muted grid, mono labels, no decoration.
 * points: [{ label, value }]
 */
export function AreaChart({ points, height = 220, ariaLabel }) {
  if (!points || points.length < 2) return null;
  const W = 640;
  const H = height;
  const pad = { top: 14, right: 10, bottom: 24, left: 44 };
  const innerW = W - pad.left - pad.right;
  const innerH = H - pad.top - pad.bottom;
  const max = niceCeil(Math.max(...points.map((point) => point.value)));
  const step = innerW / (points.length - 1);
  const x = (index) => pad.left + index * step;
  const y = (value) => pad.top + innerH - (value / max) * innerH;

  const line = points
    .map((point, index) => `${index === 0 ? "M" : "L"}${x(index).toFixed(1)},${y(point.value).toFixed(1)}`)
    .join(" ");
  const area = `${line} L${x(points.length - 1).toFixed(1)},${(pad.top + innerH).toFixed(1)} L${pad.left},${(pad.top + innerH).toFixed(1)} Z`;

  const gridValues = [max, max / 2];
  const labelEvery = Math.max(1, Math.ceil(points.length / 5));
  const xIndices = points
    .map((_, index) => index)
    .filter((index) => index % labelEvery === 0);

  return (
    <svg className="chart" viewBox={`0 0 ${W} ${H}`} role="img" aria-label={ariaLabel}>
      {gridValues.map((value) => (
        <g key={guid("grid", value)}>
          <line
            x1={pad.left}
            x2={W - pad.right}
            y1={y(value)}
            y2={y(value)}
            stroke="var(--line)"
            strokeWidth="1"
          />
          <text x={pad.left - 6} y={y(value) + 3} textAnchor="end">
            {compact.format(value)}
          </text>
        </g>
      ))}
      <line
        x1={pad.left}
        x2={W - pad.right}
        y1={pad.top + innerH}
        y2={pad.top + innerH}
        stroke="var(--line-strong)"
        strokeWidth="1"
      />
      <path d={area} fill="var(--accent)" opacity="0.08" />
      <path d={line} fill="none" stroke="var(--accent)" strokeWidth="1.8" strokeLinejoin="round" />
      {xIndices.map((index) => (
        <text key={guid("x", index)} x={x(index)} y={H - 6} textAnchor={index === 0 ? "start" : "middle"}>
          {points[index].label}
        </text>
      ))}
    </svg>
  );
}

/**
 * Quiet ranked list with hairline meters. items: [{ label, value, sub }]
 */
export function BarList({ items, empty = "Nothing recorded yet" }) {
  if (!items || items.length === 0) {
    return <p className="small subtle">{empty}</p>;
  }
  const max = Math.max(...items.map((item) => item.value), 1);
  return (
    <div className="bar-list">
      {items.map((item) => (
        <div className="bar-row" key={item.label}>
          <span className="bar-label" title={item.label}>
            {item.label}
          </span>
          <span className="meter">
            <span style={{ width: `${Math.max(2, (item.value / max) * 100)}%` }} />
          </span>
          <span className="bar-value">
            {compact.format(item.value)}
            {item.sub ? <span className="bar-sub"> · {item.sub}</span> : null}
          </span>
        </div>
      ))}
    </div>
  );
}
