"use client";

import Link from "next/link";
import { useParams } from "next/navigation";
import { useEffect, useMemo, useState } from "react";

import { api, formatDate } from "@/lib/api";
import { useApi, useToast } from "@/lib/hooks";
import ProjectTabs from "@/components/project-tabs";
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

const PAGE = 25;

export default function UsersPage() {
  const { projectId } = useParams();
  const [searchInput, setSearchInput] = useState("");
  const [search, setSearch] = useState("");
  const [offset, setOffset] = useState(0);
  const path = useMemo(() => {
    const params = new URLSearchParams({ limit: String(PAGE), offset: String(offset) });
    if (search) params.set("search", search);
    return `/projects/${projectId}/users?${params}`;
  }, [projectId, offset, search]);

  const { data, loading, error, reload } = useApi(path);
  const { toast, notify } = useToast();

  const [createOpen, setCreateOpen] = useState(false);
  const [email, setEmail] = useState("");
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState("");

  const [editing, setEditing] = useState(null);
  const [editName, setEditName] = useState("");
  const [editStatus, setEditStatus] = useState(true);
  const [editPassword, setEditPassword] = useState("");
  const [editBusy, setEditBusy] = useState(false);
  const [editError, setEditError] = useState("");

  const [deleting, setDeleting] = useState(null);
  const [deleteBusy, setDeleteBusy] = useState(false);

  useEffect(() => {
    setOffset(0);
  }, [search]);

  async function createUser(event) {
    event.preventDefault();
    setBusy(true);
    setFormError("");
    try {
      await api(`/projects/${projectId}/users`, {
        method: "POST",
        body: {
          email,
          name: name.trim() || undefined,
          password: password || undefined,
        },
      });
      setCreateOpen(false);
      setEmail("");
      setName("");
      setPassword("");
      notify("User created");
      reload();
    } catch (err) {
      setFormError(err.message);
    } finally {
      setBusy(false);
    }
  }

  function openEdit(user) {
    setEditing(user);
    setEditName(user.name || "");
    setEditStatus(user.status);
    setEditPassword("");
    setEditError("");
  }

  async function saveUser(event) {
    event.preventDefault();
    setEditBusy(true);
    setEditError("");
    try {
      await api(`/projects/${projectId}/users/${editing.$id}`, {
        method: "PATCH",
        body: {
          name: editName || undefined,
          status: editStatus,
          password: editPassword || undefined,
        },
      });
      setEditing(null);
      notify("User updated");
      reload();
    } catch (err) {
      setEditError(err.message);
    } finally {
      setEditBusy(false);
    }
  }

  async function deleteUser() {
    setDeleteBusy(true);
    try {
      await api(`/projects/${projectId}/users/${deleting.$id}`, { method: "DELETE" });
      setDeleting(null);
      notify("User deleted");
      reload();
    } catch (err) {
      notify(err.message, "danger");
    } finally {
      setDeleteBusy(false);
    }
  }

  const total = data?.total ?? 0;

  return (
    <>
      <div className="crumbs">
        <Link href="/projects">Projects</Link>
        <span className="sep">/</span>
        <Link href={`/projects/${projectId}`}>{projectId}</Link>
        <span className="sep">/</span>
        <span>users</span>
      </div>
      <PageHead
        eyebrow="Project"
        title="Users"
        description="Account records for this project. Passwords are Argon2 hashes; imported users must have a new password set."
        actions={
          <button className="btn btn-primary" onClick={() => setCreateOpen(true)}>
            <Icon name="plus" size={14} />
            Create user
          </button>
        }
      />

      <ProjectTabs projectId={projectId} />

      <section className="panel">
        <div className="panel-head">
          <h2>All users</h2>
          <form
            className="row"
            onSubmit={(event) => {
              event.preventDefault();
              setSearch(searchInput.trim());
            }}
          >
            <div className="row" style={{ gap: 6 }}>
              <input
                className="input"
                style={{ width: 220, height: 32, padding: "6px 10px" }}
                placeholder="Search email or name"
                value={searchInput}
                onChange={(event) => setSearchInput(event.target.value)}
              />
              <button className="btn btn-sm" type="submit">
                <Icon name="search" size={13} />
              </button>
              {search ? (
                <button
                  className="btn btn-sm btn-ghost"
                  type="button"
                  onClick={() => {
                    setSearchInput("");
                    setSearch("");
                  }}
                >
                  Clear
                </button>
              ) : null}
            </div>
          </form>
        </div>

        {error ? <Notice tone="danger">{error.message}</Notice> : null}

        {loading ? (
          <Loading />
        ) : data.users.length === 0 ? (
          <Empty
            title={search ? "No matching users" : "No users yet"}
            action={
              !search ? (
                <button className="btn btn-primary btn-sm" onClick={() => setCreateOpen(true)}>
                  Create a user
                </button>
              ) : null
            }
          >
            {search
              ? "Try a different search term."
              : "Users can sign in through the Appwrite account API."}
          </Empty>
        ) : (
          <>
            <div className="table-wrap">
              <table>
                <thead>
                  <tr>
                    <th>User</th>
                    <th>Status</th>
                    <th>Password</th>
                    <th>Created</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {data.users.map((user) => (
                    <tr key={user.$id}>
                      <td>
                        <span className="strong">{user.name || "Unnamed"}</span>
                        <span className="cell-sub">{user.email}</span>
                      </td>
                      <td>
                        <Badge tone={user.status ? "ok" : "muted"}>
                          {user.status ? "active" : "disabled"}
                        </Badge>
                        {user.imported ? (
                          <span style={{ marginLeft: 6 }}>
                            <Badge tone="warn">imported</Badge>
                          </span>
                        ) : null}
                      </td>
                      <td>
                        {user.hasPassword ? (
                          <span className="subtle small">set</span>
                        ) : (
                          <Badge tone="warn">reset needed</Badge>
                        )}
                      </td>
                      <td className="subtle">{formatDate(user.$createdAt)}</td>
                      <td>
                        <div className="table-actions">
                          <button className="btn btn-sm" onClick={() => openEdit(user)}>
                            <Icon name="edit" size={13} />
                            Edit
                          </button>
                          <button
                            className="btn btn-sm btn-danger"
                            onClick={() => setDeleting(user)}
                          >
                            <Icon name="trash" size={13} />
                          </button>
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div className="pagination">
              <span>
                {offset + 1}–{Math.min(offset + PAGE, total)} of {total}
              </span>
              <div className="btn-row">
                <button
                  className="btn btn-sm"
                  disabled={offset === 0}
                  onClick={() => setOffset(Math.max(0, offset - PAGE))}
                >
                  Previous
                </button>
                <button
                  className="btn btn-sm"
                  disabled={offset + PAGE >= total}
                  onClick={() => setOffset(offset + PAGE)}
                >
                  Next
                </button>
              </div>
            </div>
          </>
        )}
      </section>

      {createOpen ? (
        <Modal
          title="Create user"
          subtitle="The account can sign in immediately if a password is set."
          onClose={() => setCreateOpen(false)}
          footer={
            <>
              <button className="btn" onClick={() => setCreateOpen(false)}>
                Cancel
              </button>
              <button className="btn btn-primary" form="new-user" type="submit" disabled={busy}>
                {busy ? <Spinner dark /> : null}
                {busy ? "Creating…" : "Create user"}
              </button>
            </>
          }
        >
          <form id="new-user" className="form-grid" onSubmit={createUser}>
            <Field label="Email" htmlFor="user-email">
              <input
                id="user-email"
                className="input"
                type="email"
                required
                autoFocus
                value={email}
                onChange={(event) => setEmail(event.target.value)}
              />
            </Field>
            <Field label="Name" htmlFor="user-name">
              <input
                id="user-name"
                className="input"
                placeholder="Ada Lovelace"
                value={name}
                onChange={(event) => setName(event.target.value)}
              />
            </Field>
            <Field
              label="Password"
              hint="Optional. Minimum 8 characters. Leave blank to require an admin reset."
              htmlFor="user-password"
            >
              <input
                id="user-password"
                className="input"
                type="password"
                minLength={8}
                value={password}
                onChange={(event) => setPassword(event.target.value)}
              />
            </Field>
            {formError ? <Notice tone="danger">{formError}</Notice> : null}
          </form>
        </Modal>
      ) : null}

      {editing ? (
        <Modal
          title="Edit user"
          subtitle={editing.email}
          onClose={() => setEditing(null)}
          footer={
            <>
              <button className="btn" onClick={() => setEditing(null)}>
                Cancel
              </button>
              <button className="btn btn-primary" form="edit-user" type="submit" disabled={editBusy}>
                {editBusy ? <Spinner dark /> : null}
                {editBusy ? "Saving…" : "Save changes"}
              </button>
            </>
          }
        >
          <form id="edit-user" className="form-grid" onSubmit={saveUser}>
            <Field label="Name" htmlFor="edit-name">
              <input
                id="edit-name"
                className="input"
                value={editName}
                onChange={(event) => setEditName(event.target.value)}
              />
            </Field>
            <div className="spread">
              <div className="stack-tight">
                <span className="strong">Account active</span>
                <span className="small subtle">Disabled users cannot sign in.</span>
              </div>
              <button
                type="button"
                className={`toggle ${editStatus ? "on" : ""}`}
                onClick={() => setEditStatus(!editStatus)}
                aria-label="Toggle account status"
              />
            </div>
            <div className="divider" />
            <Field
              label="Reset password"
              hint="Optional. Invalidates all current sessions for this user."
              htmlFor="edit-password"
            >
              <input
                id="edit-password"
                className="input"
                type="password"
                minLength={8}
                placeholder="Leave blank to keep the current password"
                value={editPassword}
                onChange={(event) => setEditPassword(event.target.value)}
              />
            </Field>
            {editError ? <Notice tone="danger">{editError}</Notice> : null}
            {editing.imported && !editing.hasPassword ? (
              <Notice tone="warn">
                This user was imported from Appwrite. Their original password hash is not
                available — set a new password here.
              </Notice>
            ) : null}
          </form>
        </Modal>
      ) : null}

      {deleting ? (
        <Modal
          title="Delete user?"
          subtitle={deleting.email}
          onClose={() => setDeleting(null)}
          footer={
            <>
              <button className="btn" onClick={() => setDeleting(null)}>
                Cancel
              </button>
              <button className="btn btn-danger" onClick={deleteUser} disabled={deleteBusy}>
                {deleteBusy ? <Spinner /> : <Icon name="trash" size={13} />}
                Delete permanently
              </button>
            </>
          }
        >
          <Notice tone="danger">
            This removes the account and all of its sessions. Documents created by this
            user are not deleted.
          </Notice>
        </Modal>
      ) : null}

      <Toast message={toast?.message} tone={toast?.tone} />
    </>
  );
}
