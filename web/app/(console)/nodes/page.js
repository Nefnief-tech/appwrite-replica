"use client";

import { useEffect, useState } from "react";

import NodeGraph from "@/components/node-graph";
import { api, formatRelative } from "@/lib/api";
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

function lagLabel(status) {
  if (!status?.ok) return "offline";
  if (status.lagSeconds === 0) return "in sync";
  if (status.lagSeconds != null) return `${status.lagSeconds.toFixed(2)}s behind`;
  return "streaming";
}

export default function NodesPage() {
  const { data, loading, error, reload } = useApi("/nodes");
  const { toast, notify } = useToast();

  const [selectedNode, setSelectedNode] = useState(null);
  const [docker, setDocker] = useState(null);
  const [spawnOpen, setSpawnOpen] = useState(false);
  const [spawnName, setSpawnName] = useState("");
  const [spawnBusy, setSpawnBusy] = useState(false);
  const [spawnError, setSpawnError] = useState("");

  useEffect(() => {
    api("/docker/status").then(setDocker).catch(() => setDocker({ available: false }));
  }, []);

  const [addOpen, setAddOpen] = useState(false);
  const [name, setName] = useState("");
  const [dsn, setDsn] = useState("");
  const [role, setRole] = useState("follower");
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState("");

  const [checking, setChecking] = useState(null);
  const [promoting, setPromoting] = useState(null);
  const [promoteBusy, setPromoteBusy] = useState(false);
  const [removing, setRemoving] = useState(null);
  const [removeBusy, setRemoveBusy] = useState(false);

  async function addNode(event) {
    event.preventDefault();
    setBusy(true);
    setFormError("");
    try {
      await api("/nodes", { method: "POST", body: { name, dsn, role } });
      setAddOpen(false);
      setName("");
      setDsn("");
      setRole("follower");
      reload();
      notify("Node registered");
    } catch (err) {
      setFormError(err.message);
    } finally {
      setBusy(false);
    }
  }

  async function spawnReplica(event) {
    event.preventDefault();
    setSpawnBusy(true);
    setSpawnError("");
    try {
      await api("/docker/replicas", {
        method: "POST",
        body: { name: spawnName || undefined },
      });
      setSpawnOpen(false);
      setSpawnName("");
      reload();
      notify("Replica started and registered");
    } catch (err) {
      setSpawnError(err.message);
    } finally {
      setSpawnBusy(false);
    }
  }

  async function check(node) {
    setChecking(node.id);
    try {
      const result = await api(`/nodes/${node.id}/check`, { method: "POST" });
      reload();
      notify(
        result.status?.ok
          ? `${node.name}: healthy, ${lagLabel(result.status)}`
          : `${node.name}: ${result.status?.error || "unreachable"}`,
        result.status?.ok ? "" : "danger",
      );
    } catch (err) {
      notify(err.message, "danger");
    } finally {
      setChecking(null);
    }
  }

  async function promote() {
    setPromoteBusy(true);
    try {
      await api(`/nodes/${promoting.id}/promote`, { method: "POST" });
      setPromoting(null);
      reload();
      notify(`${promoting.name} is now the leader`);
    } catch (err) {
      notify(err.message, "danger");
    } finally {
      setPromoteBusy(false);
    }
  }

  async function remove() {
    setRemoveBusy(true);
    try {
      const managed = removing.name.startsWith("ow-replica-");
      await api(managed ? `/docker/replicas/${removing.id}` : `/nodes/${removing.id}`, {
        method: "DELETE",
      });
      setRemoving(null);
      reload();
      notify("Node removed");
    } catch (err) {
      notify(err.message, "danger");
    } finally {
      setRemoveBusy(false);
    }
  }

  return (
    <>
      <PageHead
        eyebrow="Infrastructure"
        title="Nodes"
        description="One Postgres leader takes writes; followers stream from it. Openwrite routes reads to the leader unless READ_ROUTING is set to replica."
        actions={
          <>
            <button
              className="btn btn-primary"
              onClick={() => setSpawnOpen(true)}
              disabled={docker?.available === false}
              title={docker?.available === false ? docker.error || "Docker unavailable" : undefined}
            >
              <Icon name="database" size={14} />
              Spawn replica
            </button>
            <button className="btn" onClick={() => setAddOpen(true)}>
              <Icon name="plus" size={14} />
              Register node
            </button>
          </>
        }
      />

      {error ? <Notice tone="danger">{error.message}</Notice> : null}

      {!loading && data?.nodes?.length ? (
        <section className="panel" style={{ marginBottom: 18 }}>
          <div className="panel-head">
            <h2>Replication topology</h2>
            <span className="mono">{data.nodes.length} nodes</span>
          </div>
          <NodeGraph nodes={data.nodes} selectedId={selectedNode} onSelect={setSelectedNode} />
        </section>
      ) : null}

      <section className="panel">
        <div className="panel-head">
          <h2>Database nodes</h2>
          <span className="mono">
            writer {data?.writerId ? data.writerId.slice(0, 8) : "—"}
          </span>
        </div>

        {loading ? (
          <Loading />
        ) : data.nodes.length === 0 ? (
          <Empty
            title="No nodes registered"
            action={
              <button className="btn btn-primary btn-sm" onClick={() => setAddOpen(true)}>
                Register the leader
              </button>
            }
          >
            On first boot Openwrite registers DATABASE_URL as the leader automatically
          </Empty>
        ) : (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Node</th>
                  <th>Role</th>
                  <th>Status</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {data.nodes.map((node) => (
                  <tr key={node.id} className={selectedNode === node.id ? "graph-row-selected" : ""}>
                    <td>
                      <span className="row" style={{ gap: 8 }}>
                        <span className={`dot ${node.status?.ok ? "ok" : "fail"}`} />
                        <span className="strong">{node.name}</span>
                      </span>
                      <span className="cell-sub mono">{node.dsnHint}</span>
                      <span className="cell-sub">
                        checked {node.status?.checkedAt ? formatRelative(node.status.checkedAt) : "never"}
                        {node.status?.error && !node.status?.ok ? ` · ${node.status.error}` : ""}
                      </span>
                    </td>
                    <td>
                      <span className="row" style={{ gap: 5, flexWrap: "wrap" }}>
                        <Badge
                          tone={
                            node.role === "leader" ? "dark" : node.role === "follower" ? "acid" : "muted"
                          }
                        >
                          {node.role}
                        </Badge>
                        {node.isWriter ? <Badge tone="ok">writer</Badge> : null}
                        {node.name.startsWith("ow-replica-") ? (
                          <Badge tone="acid">docker</Badge>
                        ) : null}
                      </span>
                    </td>
                    <td>
                      {node.status?.inRecovery ? (
                        <span className="small">{lagLabel(node.status)}</span>
                      ) : (
                        <span className="small subtle">
                          {node.status?.readOnly ? "read-only" : "primary"}
                        </span>
                      )}
                      <span className="cell-sub mono">
                        {node.status?.latencyMs != null ? `${node.status.latencyMs} ms` : "offline"}
                        {node.status?.lagBytes != null ? ` · ${node.status.lagBytes} B` : ""}
                      </span>
                    </td>
                    <td>
                      <div className="table-actions">
                        <button
                          className="btn btn-sm"
                          onClick={() => check(node)}
                          disabled={checking === node.id}
                        >
                          {checking === node.id ? <Spinner /> : <Icon name="refresh" size={13} />}
                          Check
                        </button>
                        {node.role === "follower" ? (
                          <button className="btn btn-sm" onClick={() => setPromoting(node)}>
                            Promote
                          </button>
                        ) : null}
                        {node.role !== "leader" ? (
                          <button
                            className="btn btn-sm btn-danger"
                            onClick={() => setRemoving(node)}
                            aria-label={`Remove ${node.name}`}
                          >
                            <Icon name="trash" size={13} />
                          </button>
                        ) : null}
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <div className="section-title">
        <h2>How failover works</h2>
        <span>Read this before promoting</span>
      </div>
      <section className="panel panel-pad">
        <div className="grid-3">
          <div>
            <h3>1 · Prepare the replica</h3>
            <p className="small subtle" style={{ margin: "4px 0 0" }}>
              Use Spawn replica to build a follower with pg_basebackup against the current
              leader, or register one you prepared yourself. Followers must be reachable
              from the API and stream continuously.
            </p>
          </div>
          <div>
            <h3>2 · Promote at the database</h3>
            <p className="small subtle" style={{ margin: "4px 0 0" }}>
              Run <span className="inline-code">pg_ctl promote</span> on the target and fence
              the old leader so it cannot accept writes. Never run two appliances at once.
            </p>
          </div>
          <div>
            <h3>3 · Promote in Openwrite</h3>
            <p className="small subtle" style={{ margin: "4px 0 0" }}>
              Openwrite verifies the target accepts writes, updates node roles, rewrites the
              encrypted leader file and reloads routing.
            </p>
          </div>
        </div>
      </section>

      {spawnOpen ? (
        <Modal
          title="Spawn Postgres replica"
          subtitle="Runs on this host's Docker daemon and registers the new node automatically"
          onClose={() => setSpawnOpen(false)}
          footer={
            <>
              <button className="btn" onClick={() => setSpawnOpen(false)}>
                Cancel
              </button>
              <button className="btn btn-primary" form="new-replica" type="submit" disabled={spawnBusy}>
                {spawnBusy ? <Spinner dark /> : null}
                {spawnBusy ? "Starting…" : "Spawn replica"}
              </button>
            </>
          }
        >
          <form id="new-replica" className="form-grid" onSubmit={spawnReplica}>
            <Field
              label="Replica name (optional)"
              hint="Container and node are named ow-replica-…; leave empty for a generated name."
              htmlFor="replica-name"
            >
              <input
                id="replica-name"
                className="input"
                autoFocus
                placeholder="eu-west-1"
                value={spawnName}
                onChange={(event) => setSpawnName(event.target.value)}
              />
            </Field>
            <Notice tone="warn">
              Docker Engine {docker?.version} is available. The API controls Docker through
              /var/run/docker.sock, so console admins can manage containers on this host.
            </Notice>
            {spawnError ? <Notice tone="danger">{spawnError}</Notice> : null}
          </form>
        </Modal>
      ) : null}

      {addOpen ? (
        <Modal
          title="Register node"
          subtitle="DSNs are encrypted with NODE_SECRET before they touch the database"
          wide
          onClose={() => setAddOpen(false)}
          footer={
            <>
              <button className="btn" onClick={() => setAddOpen(false)}>
                Cancel
              </button>
              <button className="btn btn-primary" form="new-node" type="submit" disabled={busy}>
                {busy ? <Spinner dark /> : null}
                {busy ? "Registering…" : "Register node"}
              </button>
            </>
          }
        >
          <form id="new-node" className="form-grid" onSubmit={addNode}>
            <div className="form-row">
              <Field label="Name" htmlFor="node-name">
                <input
                  id="node-name"
                  className="input"
                  required
                  autoFocus
                  placeholder="replica-eu-1"
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                />
              </Field>
              <Field label="Role" htmlFor="node-role">
                <select
                  id="node-role"
                  className="select"
                  value={role}
                  onChange={(event) => setRole(event.target.value)}
                >
                  <option value="follower">Follower (read replica)</option>
                  <option value="leader">Leader (only if none exists)</option>
                </select>
              </Field>
            </div>
            <Field
              label="PostgreSQL DSN"
              hint="postgres://user:password@host:5432/openwrite — the target must use the same schema and migrations"
              htmlFor="node-dsn"
            >
              <input
                id="node-dsn"
                className="input mono"
                required
                placeholder="postgres://openwrite:secret@10.0.0.12:5432/openwrite"
                value={dsn}
                onChange={(event) => setDsn(event.target.value)}
              />
            </Field>
            {role === "leader" ? (
              <Notice tone="warn">
                A leader can only be registered when no leader exists. Normally you add a
                follower and promote it after preparing the database.
              </Notice>
            ) : null}
            {formError ? <Notice tone="danger">{formError}</Notice> : null}
          </form>
        </Modal>
      ) : null}

      {promoting ? (
        <Modal
          title={`Promote ${promoting.name}?`}
          subtitle="Openwrite will check that this node accepts writes before switching"
          onClose={() => setPromoting(null)}
          footer={
            <>
              <button className="btn" onClick={() => setPromoting(null)}>
                Cancel
              </button>
              <button className="btn btn-primary" onClick={promote} disabled={promoteBusy}>
                {promoteBusy ? <Spinner dark /> : null}
                {promoteBusy ? "Promoting…" : "Promote to leader"}
              </button>
            </>
          }
        >
          <Notice tone="danger">
            Only promote after running <span className="inline-code">pg_ctl promote</span> on{" "}
            {promoting.name} and fencing the current leader. If both accept writes you will
            split-brain and lose data.
          </Notice>
          <p className="small subtle" style={{ margin: "14px 0 0" }}>
            The old leader is marked retired and reads move to the new leader immediately.
            The API keeps serving from its configured leader file after a restart.
          </p>
        </Modal>
      ) : null}

      {removing ? (
        <Modal
          title="Remove node?"
          subtitle={removing.name}
          onClose={() => setRemoving(null)}
          footer={
            <>
              <button className="btn" onClick={() => setRemoving(null)}>
                Cancel
              </button>
              <button className="btn btn-danger" onClick={remove} disabled={removeBusy}>
                {removeBusy ? <Spinner /> : <Icon name="trash" size={13} />}
                Remove node
              </button>
            </>
          }
        >
          <Notice tone="warn">
            This only removes the registration. The PostgreSQL server itself is untouched.
          </Notice>
        </Modal>
      ) : null}

      <Toast message={toast?.message} tone={toast?.tone} />
    </>
  );
}
