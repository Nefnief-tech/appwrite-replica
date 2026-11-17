"use client";

import Link from "next/link";
import { useParams } from "next/navigation";
import { useState } from "react";

import { api, formatDate } from "@/lib/api";
import { useApi } from "@/lib/hooks";
import ProjectTabs from "@/components/project-tabs";
import {
  CopyRow,
  Empty,
  Field,
  Icon,
  Loading,
  Modal,
  Notice,
  PageHead,
  Spinner,
} from "@/components/ui";

export default function ProjectPage() {
  const { projectId } = useParams();
  const { data, loading, error, reload } = useApi(`/projects/${projectId}`);
  const [newDb, setNewDb] = useState(false);
  const [dbName, setDbName] = useState("");
  const [dbId, setDbId] = useState("");
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState("");

  const [rotateOpen, setRotateOpen] = useState(false);
  const [rotating, setRotating] = useState(false);
  const [rotated, setRotated] = useState(null);

  async function createDatabase(event) {
    event.preventDefault();
    setBusy(true);
    setFormError("");
    try {
      await api(`/projects/${projectId}/databases`, {
        method: "POST",
        body: { name: dbName, id: dbId.trim() || undefined },
      });
      setNewDb(false);
      setDbName("");
      setDbId("");
      reload();
    } catch (err) {
      setFormError(err.message);
    } finally {
      setBusy(false);
    }
  }

  async function rotateKey() {
    setRotating(true);
    try {
      const result = await api(`/projects/${projectId}/key`, { method: "POST" });
      setRotated(result.apiKey);
      setRotateOpen(false);
      reload();
    } catch (err) {
      setFormError(err.message);
    } finally {
      setRotating(false);
    }
  }

  if (loading) return <Loading label="Loading project…" />;
  if (error) return <Notice tone="danger">{error.message}</Notice>;

  const endpoint = data.appwrite?.endpoint || "";
  const snippet = `import { Client, Databases } from "appwrite";

const client = new Client()
  .setEndpoint("${endpoint}")
  .setProject("${data.id}");

const databases = new Databases(client);`;

  return (
    <>
      <div className="crumbs">
        <Link href="/projects">Projects</Link>
        <span className="sep">/</span>
        <span>{data.id}</span>
      </div>
      <PageHead
        eyebrow="Project"
        title={data.name}
        description={`Created ${formatDate(data.createdAt)} · Appwrite-compatible API at /v1`}
        actions={
          <>
            <Link className="btn" href={`/projects/${data.id}/settings`}>
              <Icon name="settings" size={14} />
              Settings
            </Link>
            <button className="btn btn-primary" onClick={() => setNewDb(true)}>
              <Icon name="plus" size={14} />
              New database
            </button>
          </>
        }
      />

      <ProjectTabs projectId={projectId} />

      <div className="grid-3">
        <div className="stat">
          <div className="stat-label">Databases</div>
          <div className="stat-value">{data.counts.databases}</div>
          <div className="stat-note">one schema each</div>
        </div>
        <div className="stat">
          <div className="stat-label">Collections</div>
          <div className="stat-value">{data.counts.collections}</div>
          <div className="stat-note">one physical table each</div>
        </div>
        <div className="stat">
          <div className="stat-label">Users</div>
          <div className="stat-value">{data.counts.users}</div>
          <div className="stat-note">
            <Link href={`/projects/${data.id}/users`} className="link">
              Manage users
            </Link>
          </div>
        </div>
      </div>

      <div className="grid-2" style={{ marginTop: 18 }}>
        <section className="panel">
          <div className="panel-head">
            <h2>API access</h2>
            <span className="badge muted">key hidden</span>
          </div>
          <div className="panel-body" style={{ paddingTop: 6, paddingBottom: 12 }}>
            <CopyRow label="Endpoint" value={endpoint} />
            <CopyRow label="Project ID" value={data.id} />
            <CopyRow label="Key prefix" value={data.appwrite?.apiKeyPrefix || "—"} />
          </div>
          <div className="mutate-row">
            <div>
              <h3>Rotate API key</h3>
              <p className="small subtle" style={{ margin: "2px 0 0" }}>
                Invalidates the current server key immediately
              </p>
            </div>
            <button className="btn btn-sm" onClick={() => setRotateOpen(true)}>
              <Icon name="key" size={13} />
              Rotate
            </button>
          </div>
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2>Databases</h2>
            <span className="mono">{data.databases?.length ?? 0} total</span>
          </div>
          <div className="list">
            {(data.databases || []).map((database) => (
              <Link
                className="list-item"
                key={database.$id}
                href={`/projects/${data.id}/databases/${database.$id}`}
              >
                <span className="list-icon">
                  <Icon name="database" size={15} />
                </span>
                <div className="list-main">
                  <strong>{database.name}</strong>
                  <span className="mono">{database.$id}</span>
                </div>
                <Icon name="chevron-right" size={15} className="chev" />
              </Link>
            ))}
            {(!data.databases || data.databases.length === 0) ? (
              <Empty title="No databases">
                Create a database, then add collections and documents
              </Empty>
            ) : null}
          </div>
        </section>
      </div>

      <div className="section-title">
        <h2>Connect an SDK</h2>
        <span>Appwrite-compatible</span>
      </div>
      <section className="panel">
        <div className="panel-body">
          <pre className="code-block">{snippet}</pre>
          <p className="small subtle" style={{ margin: "12px 0 0" }}>
            Client SDKs can call account and document routes with a user session. Server
            SDKs pass the project API key as{" "}
            <span className="inline-code">X-Appwrite-Key</span>.
          </p>
        </div>
      </section>

      {newDb ? (
        <Modal
          title="New database"
          subtitle="Creates a real PostgreSQL schema named ow_<project>_<database>"
          onClose={() => setNewDb(false)}
          footer={
            <>
              <button className="btn" onClick={() => setNewDb(false)}>
                Cancel
              </button>
              <button className="btn btn-primary" form="new-db" type="submit" disabled={busy}>
                {busy ? <Spinner dark /> : null}
                {busy ? "Creating…" : "Create database"}
              </button>
            </>
          }
        >
          <form id="new-db" className="form-grid" onSubmit={createDatabase}>
            <Field label="Database name" htmlFor="db-name">
              <input
                id="db-name"
                className="input"
                required
                autoFocus
                placeholder="Main"
                value={dbName}
                onChange={(event) => setDbName(event.target.value)}
              />
            </Field>
            <Field
              label="Database ID"
              hint="Optional. Letters, numbers, dot, dash and underscore."
              htmlFor="db-id"
            >
              <input
                id="db-id"
                className="input mono"
                placeholder="main"
                value={dbId}
                onChange={(event) => setDbId(event.target.value)}
              />
            </Field>
            {formError ? <Notice tone="danger">{formError}</Notice> : null}
          </form>
        </Modal>
      ) : null}

      {rotateOpen ? (
        <Modal
          title="Rotate API key?"
          subtitle="Any server integration using the old key will start failing"
          onClose={() => setRotateOpen(false)}
          footer={
            <>
              <button className="btn" onClick={() => setRotateOpen(false)}>
                Cancel
              </button>
              <button className="btn btn-primary" onClick={rotateKey} disabled={rotating}>
                {rotating ? <Spinner dark /> : null}
                {rotating ? "Rotating…" : "Rotate key"}
              </button>
            </>
          }
        >
          <Notice tone="warn">
            The new key is displayed once. Update your environment variables before
            deploying.
          </Notice>
          {formError ? <Notice tone="danger">{formError}</Notice> : null}
        </Modal>
      ) : null}

      {rotated ? (
        <Modal
          title="New API key"
          subtitle="Copy it now — it cannot be retrieved later"
          onClose={() => setRotated(null)}
          footer={
            <button className="btn btn-primary" onClick={() => setRotated(null)}>
              Done
            </button>
          }
        >
          <div className="key-reveal">
            <code>{rotated}</code>
          </div>
        </Modal>
      ) : null}
    </>
  );
}
