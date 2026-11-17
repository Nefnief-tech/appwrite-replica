"use client";

import Link from "next/link";
import { useState } from "react";

import { api, formatDate } from "@/lib/api";
import { useApi } from "@/lib/hooks";
import ProjectFromJson from "@/components/project-from-json";
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

export default function ProjectsPage() {
  const { data, loading, error, reload } = useApi("/projects");
  const [open, setOpen] = useState(false);
  const [jsonOpen, setJsonOpen] = useState(false);
  const [name, setName] = useState("");
  const [id, setId] = useState("");
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState("");
  const [created, setCreated] = useState(null);

  async function create(event) {
    event.preventDefault();
    setBusy(true);
    setFormError("");
    try {
      const project = await api("/projects", {
        method: "POST",
        body: { name, id: id.trim() || undefined },
      });
      setCreated(project);
      setOpen(false);
      setName("");
      setId("");
      reload();
    } catch (err) {
      setFormError(err.message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <PageHead
        title="Projects"
        description="Each project gets its own Appwrite project ID, API key, users and logical databases"
        actions={
          <>
            <button className="btn" onClick={() => setJsonOpen(true)}>
              <Icon name="import" size={14} />
              From JSON
            </button>
            <button className="btn btn-primary" onClick={() => setOpen(true)}>
              <Icon name="plus" size={14} />
              New project
            </button>
          </>
        }
      />

      {error ? <Notice tone="danger">{error.message}</Notice> : null}

      {jsonOpen ? (
        <ProjectFromJson
          onClose={() => setJsonOpen(false)}
          onCreated={() => reload()}
        />
      ) : null}

      <section className="panel">
        <div className="panel-head">
          <h2>All projects</h2>
          <span className="mono">{data?.projects?.length ?? 0} total</span>
        </div>
        {loading ? (
          <Loading />
        ) : data.projects.length === 0 ? (
          <Empty
            title="No projects yet"
            action={
              <button className="btn btn-primary" onClick={() => setOpen(true)}>
                Create your first project
              </button>
            }
          >
            A project maps to an Appwrite project. Point your SDK at{" "}
            <span className="inline-code">/v1</span> with the project ID and API key.
          </Empty>
        ) : (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Project</th>
                  <th>Users</th>
                  <th>Databases</th>
                  <th>Collections</th>
                  <th>Created</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {data.projects.map((project) => (
                  <tr key={project.id}>
                    <td>
                      <Link href={`/projects/${project.id}`} className="strong">
                        {project.name}
                      </Link>
                      <span className="cell-sub mono">{project.id}</span>
                    </td>
                    <td>{project.counts.users}</td>
                    <td>{project.counts.databases}</td>
                    <td>{project.counts.collections}</td>
                    <td className="subtle">{formatDate(project.createdAt)}</td>
                    <td>
                      <div className="table-actions">
                        <Link className="btn btn-sm" href={`/projects/${project.id}`}>
                          Open <Icon name="arrow-right" size={13} />
                        </Link>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {open ? (
        <Modal
          title="New project"
          subtitle="Appwrite project IDs are lowercase and unique across your instance"
          onClose={() => setOpen(false)}
          footer={
            <>
              <button className="btn" onClick={() => setOpen(false)} type="button">
                Cancel
              </button>
              <button className="btn btn-primary" form="new-project" type="submit" disabled={busy}>
                {busy ? <Spinner dark /> : null}
                {busy ? "Creating…" : "Create project"}
              </button>
            </>
          }
        >
          <form id="new-project" className="form-grid" onSubmit={create}>
            <Field label="Project name" htmlFor="project-name">
              <input
                id="project-name"
                className="input"
                required
                autoFocus
                placeholder="Acme production"
                value={name}
                onChange={(event) => setName(event.target.value)}
              />
            </Field>
            <Field
              label="Project ID"
              hint="Optional. Leave blank to generate a 20-character ID."
              htmlFor="project-id"
            >
              <input
                id="project-id"
                className="input mono"
                placeholder="acme-production"
                pattern="[a-z0-9][a-z0-9_-]{0,35}"
                value={id}
                onChange={(event) => setId(event.target.value)}
              />
            </Field>
            {formError ? (
              <Notice tone="danger">{formError}</Notice>
            ) : null}
          </form>
        </Modal>
      ) : null}

      {created ? (
        <Modal
          title="Project created"
          subtitle={`${created.name} · ${created.id}`}
          onClose={() => setCreated(null)}
          footer={
            <>
              <button className="btn" onClick={() => setCreated(null)}>
                Close
              </button>
              <Link className="btn btn-primary" href={`/projects/${created.id}`}>
                Open project <Icon name="arrow-right" size={13} />
              </Link>
            </>
          }
        >
          <Notice tone="warn">
            This API key is shown once. Store it before leaving this dialog — you can
            rotate it later, but the old key stops working immediately.
          </Notice>
          <div style={{ marginTop: 14 }}>
            <div className="key-reveal">
              <code>{created.apiKey}</code>
            </div>
          </div>
          <div className="divider" />
          <CopyRow label="Endpoint" value={typeof window !== "undefined" ? window.location.origin : ""} />
          <CopyRow label="Project" value={created.id} />
        </Modal>
      ) : null}
    </>
  );
}
