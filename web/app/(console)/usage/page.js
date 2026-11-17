"use client";

import { useState } from "react";

import { useApi } from "@/lib/hooks";
import { AreaChart, BarList } from "@/components/charts";
import { Empty, Loading, Notice, PageHead } from "@/components/ui";

const RANGES = [
  { days: 1, label: "24 hours" },
  { days: 7, label: "7 days" },
  { days: 30, label: "30 days" },
];

function bucketLabel(bucket, hourly) {
  const date = new Date(bucket);
  if (Number.isNaN(date.getTime())) return "";
  if (hourly) {
    return date.toLocaleString(undefined, { weekday: "short", hour: "2-digit" });
  }
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export default function UsagePage() {
  const [days, setDays] = useState(7);
  const { data, loading, error } = useApi(`/usage?days=${days}`);

  if (loading) return <Loading label="Reading request events…" />;
  if (error) return <Notice tone="danger">{error.message}</Notice>;

  const totals = data.totals;
  const errorRate = totals.requests > 0 ? ((totals.errors / totals.requests) * 100).toFixed(1) : null;
  const points = (data.timeseries || []).map((entry) => ({
    label: bucketLabel(entry.bucket, data.hourly),
    value: entry.requests,
  }));
  const hasTraffic = totals.requests > 0;

  return (
    <>
      <PageHead
        eyebrow="Activity"
        title="Usage"
        description="API requests recorded by your own infrastructure — who calls the API, how much and from where"
        actions={
          <div className="tabs" style={{ marginBottom: 0 }}>
            {RANGES.map((range) => (
              <button
                key={range.days}
                type="button"
                className={`tab ${days === range.days ? "active" : ""}`}
                onClick={() => setDays(range.days)}
              >
                {range.label}
              </button>
            ))}
          </div>
        }
      />

      {!hasTraffic ? (
        <Notice>
          No API requests recorded in this window. Traffic appears within a minute —
          point an SDK at <span className="inline-code">/v1</span> or curl the health route from an app.
        </Notice>
      ) : null}

      <div className="grid-4" style={{ marginTop: 18 }}>
        <div className="stat">
          <div className="stat-label">Requests</div>
          <div className="stat-value">{totals.requests}</div>
          <div className="stat-note">last {data.days} days</div>
        </div>
        <div className="stat">
          <div className="stat-label">Errors</div>
          <div className="stat-value">{totals.errors}</div>
          <div className="stat-note">{errorRate === null ? "no traffic yet" : `${errorRate}% of requests`}</div>
        </div>
        <div className="stat">
          <div className="stat-label">Avg latency</div>
          <div className="stat-value">{totals.avgLatencyMs}</div>
          <div className="stat-note">milliseconds per request</div>
        </div>
        <div className="stat acid">
          <div className="stat-label">Origins</div>
          <div className="stat-value">{totals.origins}</div>
          <div className="stat-note">across {totals.projects} {totals.projects === 1 ? "project" : "projects"}</div>
        </div>
      </div>

      <div className="section-title">
        <h2>Requests</h2>
        <span>per {data.hourly ? "hour" : "day"}</span>
      </div>
      <section className="panel panel-pad">
        {hasTraffic ? (
          <AreaChart
            points={points}
            ariaLabel={`API requests over the last ${data.days} days`}
          />
        ) : (
          <Empty title="No requests recorded yet">Usage graphs fill in as your apps call the API</Empty>
        )}
      </section>

      <div className="grid-2">
        <section className="panel">
          <div className="panel-head">
            <h2>Where users come from</h2>
            <span className="mono">{totals.origins} origins</span>
          </div>
          <div className="panel-body">
            <BarList
              items={(data.origins || []).map((origin) => ({
                label: origin.origin,
                value: origin.requests,
                sub: origin.signups > 0 ? `${origin.signups} ${origin.signups === 1 ? "signup" : "signups"}` : null,
              }))}
              empty="No origins yet — requests without an Origin or Referer header stay anonymous"
            />
          </div>
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2>Projects</h2>
            <span className="mono">{totals.projects} active</span>
          </div>
          <div className="panel-body">
            <BarList
              items={(data.projects || []).map((project) => ({
                label: project.project,
                value: project.requests,
              }))}
              empty="No project-tagged traffic yet"
            />
          </div>
        </section>
      </div>

      <div className="grid-2">
        <section className="panel">
          <div className="panel-head">
            <h2>Top routes</h2>
          </div>
          <div className="panel-body">
            <BarList
              items={(data.routes || []).map((route) => ({
                label: `${route.method} ${route.route}`,
                value: route.requests,
              }))}
            />
          </div>
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2>Clients</h2>
          </div>
          <div className="panel-body">
            <BarList
              items={(data.agents || []).map((agent) => ({
                label: agent.agent,
                value: agent.requests,
              }))}
              empty="No user agents recorded yet"
            />
          </div>
        </section>
      </div>
    </>
  );
}
