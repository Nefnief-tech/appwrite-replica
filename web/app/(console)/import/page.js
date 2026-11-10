"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";

import { api } from "@/lib/api";
import { useApi } from "@/lib/hooks";
import {
  Field,
  Icon,
  Loading,
  Notice,
  PageHead,
  Spinner,
} from "@/components/ui";

const STAT_LABELS = [
  ["databases", "Databases"],
  ["collections", "Collections"],
  ["documents", "Documents"],
  ["users", "Users"],
];

export default function ImportPage() {
  const router = useRouter();
  const projects = useApi("/projects");

  const [endpoint, setEndpoint] = useState("https://cloud.appwrite.io/v1");
  const [apiKey, setApiKey] = useState("");
  const [remoteProject, setRemoteProject] = useState("");
  const [target, setTarget] = useState("new");
  const [targetName, setTargetName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState(null);

  async function runImport(event) {
    event.preventDefault();
    setBusy(true);
    setError("");
    setResult(null);
    try {
      const body = {
        endpoint,
        apiKey,
        remoteProjectId: remoteProject,
      };
      if (target !== "new") {
        body.targetProjectId = target;
      } else if (targetName.trim()) {
        body.targetProjectName = targetName.trim();
      }
      const response = await api("/import", { method: "POST", body });
      setResult(response);
    } catch (err) {
      setError(err.message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <PageHead
        eyebrow="Migration"
        title="Import from Appwrite"
        description="Copy databases, collections, documents and users from Appwrite Cloud or any self-hosted Appwrite into Openwrite."
      />

      <div className="grid-2">
        <section className="panel">
          <div className="panel-head">
            <h2>Connection</h2>
            <span className="badge muted">Appwrite API v1</span>
          </div>
          <div className="panel-body">
            <form id="import-form" className="form-grid" onSubmit={runImport}>
              <Field
                label="Appwrite endpoint"
                hint="Cloud is https://cloud.appwrite.io/v1. Self-hosted: your /v1 URL."
                htmlFor="import-endpoint"
              >
                <input
                  id="import-endpoint"
                  className="input mono"
                  required
                  value={endpoint}
                  onChange={(event) => setEndpoint(event.target.value)}
                />
              </Field>
              <Field
                label="Appwrite API key"
                hint="Create a server key with databases, collections, documents and users scopes."
                htmlFor="import-key"
              >
                <input
                  id="import-key"
                  className="input mono"
                  type="password"
                  required
                  autoComplete="off"
                  placeholder="standard_…"
                  value={apiKey}
                  onChange={(event) => setApiKey(event.target.value)}
                />
              </Field>
              <Field
                label="Appwrite project ID"
                hint="The source project. Its ID is shown in the Appwrite console."
                htmlFor="import-project"
              >
                <input
                  id="import-project"
                  className="input mono"
                  required
                  placeholder="my-app"
                  value={remoteProject}
                  onChange={(event) => setRemoteProject(event.target.value)}
                />
              </Field>

              <div className="divider" />

              <Field label="Destination" htmlFor="import-target">
                {projects.loading ? (
                  <Loading label="Loading projects…" />
                ) : (
                  <select
                    id="import-target"
                    className="select"
                    value={target}
                    onChange={(event) => setTarget(event.target.value)}
                  >
                    <option value="new">Create a new Openwrite project</option>
                    {(projects.data?.projects || []).map((project) => (
                      <option key={project.id} value={project.id}>
                        Merge into {project.name} ({project.id})
                      </option>
                    ))}
                  </select>
                )}
              </Field>

              {target === "new" ? (
                <Field
                  label="New project name"
                  hint="Optional. Defaults to the Appwrite project ID."
                  htmlFor="import-name"
                >
                  <input
                    id="import-name"
                    className="input"
                    placeholder="My app (imported)"
                    value={targetName}
                    onChange={(event) => setTargetName(event.target.value)}
                  />
                </Field>
              ) : null}

              {error ? <Notice tone="danger">{error}</Notice> : null}
            </form>
          </div>
          <div className="mutate-row">
            <div>
              <h3>Start import</h3>
              <p className="small subtle" style={{ margin: "2px 0 0" }}>
                Existing IDs are skipped, so a failed run can be retried.
              </p>
            </div>
            <button className="btn btn-primary" form="import-form" type="submit" disabled={busy}>
              {busy ? <Spinner dark /> : <Icon name="import" size={14} />}
              {busy ? "Importing…" : "Import project"}
            </button>
          </div>
        </section>

        <div className="stack">
          <section className="panel panel-pad">
            <h2 style={{ fontSize: 21, marginBottom: 10 }}>What is copied</h2>
            <div className="stack-tight small">
              <span>
                <strong>Databases and collections</strong> — recreated with their IDs and
                permissions; each becomes a real Postgres schema and table.
              </span>
              <span>
                <strong>Documents</strong> — inserted directly with original IDs, JSONB data,
                ACLs and timestamps. Up to the configured document limit.
              </span>
              <span>
                <strong>Users</strong> — accounts are recreated and marked{" "}
                <span className="inline-code">imported</span>. Password hashes are never
                exposed by Appwrite.
              </span>
              <span>
                <strong>Attributes</strong> — supported scalar types are mapped. Relationships,
                spatial and other advanced types are skipped.
              </span>
            </div>
          </section>

          <section className="panel panel-pad">
            <h2 style={{ fontSize: 21, marginBottom: 10 }}>Passwords after import</h2>
            <Notice tone="warn">
              Imported users have no password. They cannot sign in until an admin sets one in
              the project&apos;s Users view, or you trigger a password recovery flow once that
              exists. Plan a reset email before cutover.
            </Notice>
          </section>

          <section className="panel panel-pad">
            <h2 style={{ fontSize: 21, marginBottom: 10 }}>Cutover checklist</h2>
            <ol className="small subtle" style={{ margin: 0, paddingLeft: 18, lineHeight: 1.8 }}>
              <li>Freeze writes in the source app or accept a short delta.</li>
              <li>Run the import and check skipped documents/attributes.</li>
              <li>Point your SDK endpoint at Openwrite and swap the project ID.</li>
              <li>Rotate the Openwrite API key into your server secrets.</li>
              <li>Set passwords for imported users before enabling sign-in.</li>
            </ol>
          </section>
        </div>
      </div>

      {result ? (
        <div className="section-title">
          <h2>Import finished</h2>
          <span className="mono">{result.projectId}</span>
        </div>
      ) : null}

      {result ? (
        <section className="panel">
          <div className="panel-head">
            <h2>Summary</h2>
            {result.truncated ? (
              <span className="badge warn">document limit reached</span>
            ) : (
              <span className="badge ok">complete</span>
            )}
          </div>
          <div className="panel-body">
            <div className="grid-4">
              {STAT_LABELS.map(([key, label]) => (
                <div className="stat" key={key}>
                  <div className="stat-label">{label}</div>
                  <div className="stat-value">{result.stats[key] ?? 0}</div>
                </div>
              ))}
            </div>

            <div className="divider" />

            <div className="kv">
              <div>
                <span>Project ID</span>
                <strong className="mono">{result.projectId}</strong>
              </div>
              <div>
                <span>Skipped attributes</span>
                <strong>{result.stats.skippedAttributes}</strong>
              </div>
              <div>
                <span>Skipped documents</span>
                <strong>{result.stats.skippedDocuments}</strong>
              </div>
              <div>
                <span>Skipped users</span>
                <strong>{result.stats.skippedUsers}</strong>
              </div>
            </div>

            {result.apiKey ? (
              <>
                <div className="divider" />
                <Notice tone="warn">
                  This API key is shown once. Store it now — it cannot be retrieved later.
                </Notice>
                <div className="key-reveal" style={{ marginTop: 12 }}>
                  <code>{result.apiKey}</code>
                </div>
              </>
            ) : null}

            {result.truncated ? (
              <div style={{ marginTop: 14 }}>
                <Notice tone="warn">
                  The import stopped at the configured document limit. Run it again to copy
                  the remaining documents; existing rows are skipped.
                </Notice>
              </div>
            ) : null}

            <div className="btn-row" style={{ marginTop: 18 }}>
              <button className="btn btn-primary" onClick={() => router.push(`/projects/${result.projectId}`)}>
                Open project <Icon name="arrow-right" size={13} />
              </button>
              <button className="btn" onClick={() => router.refresh()}>
                Import again
              </button>
            </div>
          </div>
        </section>
      ) : null}
    </>
  );
}
