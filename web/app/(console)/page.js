"use client";

import Link from "next/link";
import { useEffect, useState } from "react";

import { compactNumber, formatRelative, health } from "@/lib/api";
import { useApi } from "@/lib/hooks";
import {
  Badge,
  Empty,
  Icon,
  Loading,
  Notice,
  PageHead,
} from "@/components/ui";

export default function OverviewPage() {
  const { data, loading, error } = useApi("/overview");
  const [system, setSystem] = useState(null);

  useEffect(() => {
    health()
      .then(setSystem)
      .catch(() => setSystem({ status: "fail" }));
  }, []);

  if (loading) return <Loading label="Reading workspace state…" />;
  if (error) {
    return (
      <Notice tone="danger">
        Could not load the workspace: {error.message}
      </Notice>
    );
  }

  const totals = data.totals;
  const projects = data.projects.slice(0, 4);
  const nodes = data.nodes;

  return (
    <>
      <PageHead
        eyebrow="Console"
        title="Overview"
        description="Projects, data and database nodes at a glance. Everything here lives on your infrastructure."
        actions={
          <Link className="btn btn-primary" href="/projects">
            <Icon name="plus" size={14} />
            New project
          </Link>
        }
      />

      <div className="grid-4">
        <div className="stat">
          <div className="stat-label">Projects</div>
          <div className="stat-value">{totals.projects}</div>
          <div className="stat-note">
            {compactNumber(totals.users)} {totals.users === 1 ? "user" : "users"} in total
          </div>
        </div>
        <div className="stat">
          <div className="stat-label">Documents</div>
          <div className="stat-value">{compactNumber(totals.documents)}</div>
          <div className="stat-note">
            across {totals.collections} collections
          </div>
        </div>
        <div className="stat">
          <div className="stat-label">Databases</div>
          <div className="stat-value">{totals.databases}</div>
          <div className="stat-note">logical databases, real SQL schemas</div>
        </div>
        <div className="stat acid">
          <div className="stat-label">Database nodes</div>
          <div className="stat-value">{nodes.length}</div>
          <div className="stat-note">
            {nodes.filter((node) => node.status?.ok).length} healthy ·{" "}
            {nodes.filter((node) => node.role === "follower").length} replicas
          </div>
        </div>
      </div>

      <div className="section-title">
        <h2>Workspace</h2>
        <span>{system?.status === "pass" ? "All services online" : "Checking services"}</span>
      </div>

      <div className="grid-2">
        <section className="panel">
          <div className="panel-head">
            <h2>Nodes</h2>
            <Link className="btn btn-sm btn-ghost" href="/nodes">
              Manage <Icon name="arrow-right" size={13} />
            </Link>
          </div>
          <div className="list">
            {nodes.slice(0, 4).map((node) => (
              <div className="list-item" key={node.id}>
                <span className={`dot ${node.status?.ok ? "ok" : "fail"}`} />
                <div className="list-main">
                  <strong>{node.name}</strong>
                  <span className="mono">
                    {node.role}
                    {node.isWriter ? " · writer" : ""}
                  </span>
                </div>
                <div style={{ textAlign: "right" }}>
                  {node.role === "leader" ? (
                    <Badge tone="dark">leader</Badge>
                  ) : node.role === "follower" ? (
                    <Badge tone="acid">follower</Badge>
                  ) : (
                    <Badge tone="muted">retired</Badge>
                  )}
                  <span className="small subtle" style={{ display: "block", marginTop: 4 }}>
                    {node.status?.latencyMs != null ? `${node.status.latencyMs} ms` : "offline"}
                  </span>
                </div>
              </div>
            ))}
            {nodes.length === 0 ? (
              <Empty title="No nodes registered">Add a follower to build redundancy</Empty>
            ) : null}
          </div>
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2>Projects</h2>
            <Link className="btn btn-sm btn-ghost" href="/projects">
              All projects <Icon name="arrow-right" size={13} />
            </Link>
          </div>
          <div className="list">
            {projects.map((project) => (
              <Link className="list-item" href={`/projects/${project.id}`} key={project.id}>
                <span className="list-icon">
                  <Icon name="database" size={16} />
                </span>
                <div className="list-main">
                  <strong>{project.name}</strong>
                  <span className="mono">{project.id}</span>
                </div>
                <div style={{ textAlign: "right" }}>
                  <span className="small subtle">
                    {project.counts.databases} db · {project.counts.users} users
                  </span>
                  <Icon name="chevron-right" size={15} className="chev" />
                </div>
              </Link>
            ))}
            {projects.length === 0 ? (
              <Empty
                title="No projects yet"
                action={
                  <Link className="btn btn-sm btn-primary" href="/projects">
                    Create a project
                  </Link>
                }
              >
                Openwrite serves one Appwrite project per project ID
              </Empty>
            ) : null}
          </div>
        </section>
      </div>

      <div className="section-title">
        <h2>Quick actions</h2>
        <span>Operations</span>
      </div>

      <div className="grid-3">
        <Link className="panel panel-pad" href="/projects">
          <span className="list-icon">
            <Icon name="projects" size={16} />
          </span>
          <h3 style={{ marginTop: 14 }}>Create a project</h3>
          <p className="small subtle" style={{ margin: "4px 0 0" }}>
            Get an API key and an Appwrite-compatible endpoint in seconds
          </p>
        </Link>
        <Link className="panel panel-pad" href="/import">
          <span className="list-icon">
            <Icon name="import" size={16} />
          </span>
          <h3 style={{ marginTop: 14 }}>Migrate from Appwrite Cloud</h3>
          <p className="small subtle" style={{ margin: "4px 0 0" }}>
            Copy databases, collections, documents and users into Openwrite
          </p>
        </Link>
        <Link className="panel panel-pad" href="/nodes">
          <span className="list-icon">
            <Icon name="nodes" size={16} />
          </span>
          <h3 style={{ marginTop: 14 }}>Add a replica</h3>
          <p className="small subtle" style={{ margin: "4px 0 0" }}>
            Register followers, watch replication lag and promote on failover
          </p>
        </Link>
      </div>

      <div
        className="row small subtle"
        style={{ marginTop: 30, fontFamily: "var(--mono)", fontSize: 10 }}
      >
        <span className={`dot ${system?.status === "pass" ? "ok" : "fail"}`} />
        API {system?.status === "pass" ? "healthy" : "unreachable"}
        <span style={{ color: "var(--line-strong)" }}>/</span>
        v{system?.version || "—"}
        <span style={{ color: "var(--line-strong)" }}>/</span>
        last node sweep {formatRelative(nodes[0]?.status?.checkedAt) || "pending"}
      </div>
    </>
  );
}
