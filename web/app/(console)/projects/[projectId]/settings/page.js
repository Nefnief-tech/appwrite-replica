"use client";

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useState } from "react";

import { api, formatDate } from "@/lib/api";
import { useApi } from "@/lib/hooks";
import ProjectTabs from "@/components/project-tabs";
import {
  CopyRow,
  Field,
  Icon,
  Loading,
  Modal,
  Notice,
  PageHead,
  Spinner,
} from "@/components/ui";

export default function ProjectSettingsPage() {
  const { projectId } = useParams();
  const router = useRouter();
  const { data, loading, error, reload } = useApi(`/projects/${projectId}`);

  const [rotateOpen, setRotateOpen] = useState(false);
  const [rotating, setRotating] = useState(false);
  const [newKey, setNewKey] = useState(null);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [confirm, setConfirm] = useState("");
  const [deleting, setDeleting] = useState(false);
  const [actionError, setActionError] = useState("");

  if (loading) return <Loading label="Loading project settings…" />;
  if (error) return <Notice tone="danger">{error.message}</Notice>;

  async function rotate() {
    setRotating(true);
    setActionError("");
    try {
      const result = await api(`/projects/${projectId}/key`, { method: "POST" });
      setNewKey(result.apiKey);
      setRotateOpen(false);
      reload();
    } catch (err) {
      setActionError(err.message);
    } finally {
      setRotating(false);
    }
  }

  async function deleteProject() {
    setDeleting(true);
    setActionError("");
    try {
      await api(`/projects/${projectId}`, { method: "DELETE" });
      router.replace("/projects");
    } catch (err) {
      setActionError(err.message);
      setDeleting(false);
    }
  }

  return (
    <>
      <div className="crumbs">
        <Link href="/projects">Projects</Link>
        <span className="sep">/</span>
        <Link href={`/projects/${projectId}`}>{projectId}</Link>
        <span className="sep">/</span>
        <span>settings</span>
      </div>
      <PageHead
        eyebrow="Project"
        title="Settings"
        description="Connection details, key rotation and destructive actions."
      />

      <ProjectTabs projectId={projectId} />

      <div className="stack">
        <section className="panel">
          <div className="panel-head">
            <h2>Connection</h2>
            <span className="badge muted">{data.id}</span>
          </div>
          <div className="panel-body" style={{ paddingTop: 6, paddingBottom: 12 }}>
            <CopyRow label="Endpoint" value={data.appwrite?.endpoint || ""} />
            <CopyRow label="Project ID" value={data.id} />
            <CopyRow label="Key prefix" value={data.appwrite?.apiKeyPrefix || "—"} />
          </div>
          <div className="mutate-row">
            <div>
              <h3>Rotate API key</h3>
              <p className="small subtle" style={{ margin: "2px 0 0" }}>
                Generates a new server key. The previous key stops working immediately.
              </p>
            </div>
            <button className="btn btn-sm" onClick={() => setRotateOpen(true)}>
              <Icon name="key" size={13} />
              Rotate key
            </button>
          </div>
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2>Details</h2>
          </div>
          <div className="panel-body">
            <div className="kv">
              <div>
                <span>Name</span>
                <strong>{data.name}</strong>
              </div>
              <div>
                <span>Project ID</span>
                <strong className="mono">{data.id}</strong>
              </div>
              <div>
                <span>Created</span>
                <strong>{formatDate(data.createdAt)}</strong>
              </div>
              <div>
                <span>Updated</span>
                <strong>{formatDate(data.updatedAt)}</strong>
              </div>
              <div>
                <span>Databases</span>
                <strong>{data.counts.databases}</strong>
              </div>
              <div>
                <span>Collections</span>
                <strong>{data.counts.collections}</strong>
              </div>
              <div>
                <span>Users</span>
                <strong>{data.counts.users}</strong>
              </div>
            </div>
          </div>
        </section>

        <section className="panel" style={{ borderColor: "#d9b6ac" }}>
          <div className="panel-head" style={{ borderColor: "#e4cfc9" }}>
            <h2 style={{ color: "var(--danger)" }}>Danger zone</h2>
          </div>
          <div className="panel-body">
            <div className="spread">
              <div>
                <h3>Delete this project</h3>
                <p className="small subtle" style={{ margin: "3px 0 0", maxWidth: 520 }}>
                  Drops every database schema, collection table, document, user and API key
                  belonging to <span className="inline-code">{data.id}</span>. This cannot
                  be undone.
                </p>
              </div>
              <button className="btn btn-danger" onClick={() => setDeleteOpen(true)}>
                <Icon name="trash" size={13} />
                Delete project
              </button>
            </div>
            {actionError ? (
              <div style={{ marginTop: 14 }}>
                <Notice tone="danger">{actionError}</Notice>
              </div>
            ) : null}
          </div>
        </section>
      </div>

      {rotateOpen ? (
        <Modal
          title="Rotate API key?"
          subtitle="Server integrations using the old key will fail."
          onClose={() => setRotateOpen(false)}
          footer={
            <>
              <button className="btn" onClick={() => setRotateOpen(false)}>
                Cancel
              </button>
              <button className="btn btn-primary" onClick={rotate} disabled={rotating}>
                {rotating ? <Spinner dark /> : null}
                {rotating ? "Rotating…" : "Rotate key"}
              </button>
            </>
          }
        >
          <Notice tone="warn">
            The new key is displayed once. Update your deployment secrets immediately.
          </Notice>
        </Modal>
      ) : null}

      {newKey ? (
        <Modal
          title="New API key"
          subtitle="Store it now — this is the only time it is shown."
          onClose={() => setNewKey(null)}
          footer={
            <button className="btn btn-primary" onClick={() => setNewKey(null)}>
              Done
            </button>
          }
        >
          <div className="key-reveal">
            <code>{newKey}</code>
          </div>
        </Modal>
      ) : null}

      {deleteOpen ? (
        <Modal
          title="Delete project permanently"
          subtitle={data.name}
          onClose={() => {
            setDeleteOpen(false);
            setConfirm("");
          }}
          footer={
            <>
              <button
                className="btn"
                onClick={() => {
                  setDeleteOpen(false);
                  setConfirm("");
                }}
              >
                Cancel
              </button>
              <button
                className="btn btn-danger"
                disabled={confirm !== data.id || deleting}
                onClick={deleteProject}
              >
                {deleting ? <Spinner /> : <Icon name="trash" size={13} />}
                Delete everything
              </button>
            </>
          }
        >
          <Notice tone="danger">
            All schemas named <span className="inline-code">ow_*</span> for this project
            will be dropped with CASCADE. Make a logical backup first.
          </Notice>
          <div style={{ marginTop: 16 }}>
            <Field label={`Type ${data.id} to confirm`} htmlFor="confirm-project">
              <input
                id="confirm-project"
                className="input mono"
                autoComplete="off"
                value={confirm}
                onChange={(event) => setConfirm(event.target.value)}
              />
            </Field>
          </div>
        </Modal>
      ) : null}
    </>
  );
}
