"use client";

import Link from "next/link";
import { useParams } from "next/navigation";
import { useState } from "react";

import { api } from "@/lib/api";
import { useApi, useToast } from "@/lib/hooks";
import {
  Badge,
  Empty,
  Field,
  Icon,
  Loading,
  Modal,
  Notice,
  PageHead,
  Spinner,
  Toast,
} from "@/components/ui";

const ACTIONS = ["read", "create", "update", "delete"];
const ROLES = ["any", "guests", "users"];

function defaultPermissions() {
  return {
    read: { any: true, guests: false, users: true },
    create: { any: false, guests: false, users: true },
    update: { any: false, guests: false, users: true },
    delete: { any: false, guests: false, users: true },
  };
}

function toPermissionStrings(matrix) {
  const out = [];
  for (const action of ACTIONS) {
    for (const role of ROLES) {
      if (matrix[action]?.[role]) out.push(`${action}("${role}")`);
    }
  }
  return out;
}

export default function DatabasePage() {
  const { projectId, databaseId } = useParams();
  const project = useApi(`/projects/${projectId}`);
  const collections = useApi(
    `/projects/${projectId}/databases/${databaseId}/collections`,
  );
  const { toast, notify } = useToast();

  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [id, setId] = useState("");
  const [secure, setSecure] = useState(false);
  const [matrix, setMatrix] = useState(defaultPermissions);
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState("");
  const [deleting, setDeleting] = useState(null);
  const [deleteBusy, setDeleteBusy] = useState(false);

  const database = project.data?.databases?.find((item) => item.$id === databaseId);

  function toggle(action, role) {
    setMatrix((current) => ({
      ...current,
      [action]: {
        ...current[action],
        [role]: !current[action][role],
      },
    }));
  }

  async function createCollection(event) {
    event.preventDefault();
    setBusy(true);
    setFormError("");
    try {
      await api(`/projects/${projectId}/databases/${databaseId}/collections`, {
        method: "POST",
        body: {
          name,
          id: id.trim() || undefined,
          documentSecurity: secure,
          permissions: toPermissionStrings(matrix),
        },
      });
      setOpen(false);
      setName("");
      setId("");
      setSecure(false);
      setMatrix(defaultPermissions());
      collections.reload();
      notify("Collection created");
    } catch (err) {
      setFormError(err.message);
    } finally {
      setBusy(false);
    }
  }

  async function deleteCollection() {
    setDeleteBusy(true);
    try {
      await api(
        `/projects/${projectId}/databases/${databaseId}/collections/${deleting.$id}`,
        { method: "DELETE" },
      );
      setDeleting(null);
      collections.reload();
      notify("Collection deleted");
    } catch (err) {
      notify(err.message, "danger");
    } finally {
      setDeleteBusy(false);
    }
  }

  return (
    <>
      <div className="crumbs">
        <Link href="/projects">Projects</Link>
        <span className="sep">/</span>
        <Link href={`/projects/${projectId}`}>{projectId}</Link>
        <span className="sep">/</span>
        <span>{databaseId}</span>
      </div>
      <PageHead
        eyebrow="Database"
        title={database?.name || databaseId}
        description={`Schema ow_${projectId}_${databaseId} · collections become plain PostgreSQL tables.`}
        actions={
          <button className="btn btn-primary" onClick={() => setOpen(true)}>
            <Icon name="plus" size={14} />
            New collection
          </button>
        }
      />

      <section className="panel">
        <div className="panel-head">
          <h2>Collections</h2>
          <span className="mono">{collections.data?.total ?? 0} total</span>
        </div>

        {collections.error ? (
          <Notice tone="danger">{collections.error.message}</Notice>
        ) : null}

        {collections.loading ? (
          <Loading />
        ) : collections.data.collections.length === 0 ? (
          <Empty
            title="No collections yet"
            action={
              <button className="btn btn-primary btn-sm" onClick={() => setOpen(true)}>
                Create a collection
              </button>
            }
          >
            Collections define attributes and create a real table with JSONB document data.
          </Empty>
        ) : (
          <div className="list">
            {collections.data.collections.map((collection) => (
              <div className="list-item" key={collection.$id}>
                <span className="list-icon">
                  <Icon name="collection" size={15} />
                </span>
                <div className="list-main">
                  <Link
                    href={`/projects/${projectId}/databases/${databaseId}/collections/${collection.$id}`}
                    className="strong"
                  >
                    {collection.name}
                  </Link>
                  <span className="mono">{collection.$id}</span>
                </div>
                <div className="row" style={{ gap: 8 }}>
                  <Badge>{collection.attributes.length} attrs</Badge>
                  <Badge tone={collection.documentSecurity ? "dark" : "muted"}>
                    {collection.documentSecurity ? "doc security" : "collection ACL"}
                  </Badge>
                </div>
                <div className="table-actions">
                  <Link
                    className="btn btn-sm"
                    href={`/projects/${projectId}/databases/${databaseId}/collections/${collection.$id}`}
                  >
                    Open <Icon name="arrow-right" size={13} />
                  </Link>
                  <button
                    className="btn btn-sm btn-danger"
                    onClick={() => setDeleting(collection)}
                    aria-label={`Delete ${collection.name}`}
                  >
                    <Icon name="trash" size={13} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      {open ? (
        <Modal
          title="New collection"
          subtitle="Attributes can be added after the collection is created."
          wide
          onClose={() => setOpen(false)}
          footer={
            <>
              <button className="btn" onClick={() => setOpen(false)}>
                Cancel
              </button>
              <button className="btn btn-primary" form="new-collection" type="submit" disabled={busy}>
                {busy ? <Spinner dark /> : null}
                {busy ? "Creating…" : "Create collection"}
              </button>
            </>
          }
        >
          <form id="new-collection" className="form-grid" onSubmit={createCollection}>
            <div className="form-row">
              <Field label="Collection name" htmlFor="collection-name">
                <input
                  id="collection-name"
                  className="input"
                  required
                  autoFocus
                  placeholder="Notes"
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                />
              </Field>
              <Field
                label="Collection ID"
                hint="Optional. Becomes the table name."
                htmlFor="collection-id"
              >
                <input
                  id="collection-id"
                  className="input mono"
                  placeholder="notes"
                  value={id}
                  onChange={(event) => setId(event.target.value)}
                />
              </Field>
            </div>

            <div className="spread">
              <div className="stack-tight">
                <span className="strong">Document-level security</span>
                <span className="small subtle">
                  When on, each document&apos;s own permissions decide access. Collection
                  permissions still gate create.
                </span>
              </div>
              <button
                type="button"
                className={`toggle ${secure ? "on" : ""}`}
                onClick={() => setSecure(!secure)}
                aria-label="Toggle document security"
              />
            </div>

            <div>
              <div className="small" style={{ fontWeight: 500, marginBottom: 7 }}>
                Collection permissions
              </div>
              <div className="perm-matrix">
                <div className="perm-row">
                  <span>Action</span>
                  <span>Any</span>
                  <span>Guests</span>
                  <span>Users</span>
                </div>
                {ACTIONS.map((action) => (
                  <div className="perm-row" key={action}>
                    <span style={{ textTransform: "capitalize" }}>{action}</span>
                    {ROLES.map((role) => (
                      <label className="check" key={role}>
                        <input
                          type="checkbox"
                          checked={matrix[action][role]}
                          onChange={() => toggle(action, role)}
                        />
                      </label>
                    ))}
                  </div>
                ))}
              </div>
              <div className="hint" style={{ marginTop: 7 }}>
                API keys always bypass permissions. Server SDKs use the project API key.
              </div>
            </div>

            {formError ? <Notice tone="danger">{formError}</Notice> : null}
          </form>
        </Modal>
      ) : null}

      {deleting ? (
        <Modal
          title="Delete collection?"
          subtitle={`${deleting.name} · ${deleting.$id}`}
          onClose={() => setDeleting(null)}
          footer={
            <>
              <button className="btn" onClick={() => setDeleting(null)}>
                Cancel
              </button>
              <button className="btn btn-danger" onClick={deleteCollection} disabled={deleteBusy}>
                {deleteBusy ? <Spinner /> : <Icon name="trash" size={13} />}
                Delete table and documents
              </button>
            </>
          }
        >
          <Notice tone="danger">
            The physical table <span className="inline-code">{deleting.$id}</span> and all
            of its documents will be dropped. This cannot be undone.
          </Notice>
        </Modal>
      ) : null}

      <Toast message={toast?.message} tone={toast?.tone} />
    </>
  );
}
