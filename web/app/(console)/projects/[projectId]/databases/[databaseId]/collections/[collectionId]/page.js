"use client";

import Link from "next/link";
import { useParams } from "next/navigation";
import { useMemo, useState } from "react";

import { api, formatDate } from "@/lib/api";
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

const PAGE = 25;
const TYPES = ["string", "integer", "float", "boolean", "datetime", "email", "url", "ip", "enum"];

function stripSystem(document) {
  const data = {};
  for (const [key, value] of Object.entries(document || {})) {
    if (!key.startsWith("$")) data[key] = value;
  }
  return data;
}

function cell(value) {
  if (value === null || value === undefined) return <span className="subtle">—</span>;
  if (typeof value === "object") return <span className="json-cell">{JSON.stringify(value)}</span>;
  if (typeof value === "boolean") return <Badge tone={value ? "ok" : "muted"}>{String(value)}</Badge>;
  return <span style={{ overflowWrap: "anywhere" }}>{String(value)}</span>;
}

export default function CollectionPage() {
  const { projectId, databaseId, collectionId } = useParams();
  const collection = useApi(
    `/projects/${projectId}/databases/${databaseId}/collections/${collectionId}`,
  );
  const [offset, setOffset] = useState(0);
  const documentsPath = useMemo(() => {
    const params = new URLSearchParams({
      limit: String(PAGE),
      offset: String(offset),
    });
    params.append("queries[]", 'orderDesc("$createdAt")');
    return `/projects/${projectId}/databases/${databaseId}/collections/${collectionId}/documents?${params}`;
  }, [projectId, databaseId, collectionId, offset]);
  const documents = useApi(documentsPath);
  const { toast, notify } = useToast();

  const [attributeOpen, setAttributeOpen] = useState(false);
  const [kind, setKind] = useState("string");
  const [key, setKey] = useState("");
  const [required, setRequired] = useState(false);
  const [array, setArray] = useState(false);
  const [size, setSize] = useState("");
  const [defaultValue, setDefaultValue] = useState("");
  const [elements, setElements] = useState("");
  const [attributeBusy, setAttributeBusy] = useState(false);
  const [attributeError, setAttributeError] = useState("");
  const [removingAttribute, setRemovingAttribute] = useState(null);
  const [attributeDeleteBusy, setAttributeDeleteBusy] = useState(false);

  const [docOpen, setDocOpen] = useState(false);
  const [editingDocument, setEditingDocument] = useState(null);
  const [documentId, setDocumentId] = useState("");
  const [json, setJson] = useState("");
  const [permissions, setPermissions] = useState("");
  const [documentBusy, setDocumentBusy] = useState(false);
  const [documentError, setDocumentError] = useState("");
  const [deletingDocument, setDeletingDocument] = useState(null);
  const [documentDeleteBusy, setDocumentDeleteBusy] = useState(false);

  if (collection.loading) return <Loading label="Loading collection…" />;
  if (collection.error) return <Notice tone="danger">{collection.error.message}</Notice>;

  const data = collection.data;
  const attributes = data.attributes || [];
  const total = documents.data?.total ?? 0;
  const rows = documents.data?.documents || [];
  const columns = attributes.slice(0, 4);

  function openCreateDocument() {
    setEditingDocument(null);
    setDocumentId("");
    setJson("{\n  \n}");
    setPermissions("");
    setDocumentError("");
    setDocOpen(true);
  }

  function openEditDocument(document) {
    setEditingDocument(document);
    setDocumentId(document.$id);
    setJson(JSON.stringify(stripSystem(document), null, 2));
    setPermissions((document.$permissions || []).join("\n"));
    setDocumentError("");
    setDocOpen(true);
  }

  async function saveDocument(event) {
    event.preventDefault();
    setDocumentBusy(true);
    setDocumentError("");
    let parsed;
    try {
      parsed = json.trim() ? JSON.parse(json) : {};
    } catch (err) {
      setDocumentError(`Invalid JSON: ${err.message}`);
      setDocumentBusy(false);
      return;
    }
    const body = {
      data: parsed,
      permissions: permissions.trim()
        ? permissions
            .split("\n")
            .map((line) => line.trim())
            .filter(Boolean)
        : undefined,
    };
    try {
      if (editingDocument) {
        await api(
          `/projects/${projectId}/databases/${databaseId}/collections/${collectionId}/documents/${editingDocument.$id}`,
          { method: "PATCH", body },
        );
        notify("Document updated");
      } else {
        await api(
          `/projects/${projectId}/databases/${databaseId}/collections/${collectionId}/documents`,
          { method: "POST", body: { ...body, id: documentId.trim() || undefined } },
        );
        notify("Document created");
      }
      setDocOpen(false);
      documents.reload();
    } catch (err) {
      setDocumentError(err.message);
    } finally {
      setDocumentBusy(false);
    }
  }

  async function deleteDocument() {
    setDocumentDeleteBusy(true);
    try {
      await api(
        `/projects/${projectId}/databases/${databaseId}/collections/${collectionId}/documents/${deletingDocument.$id}`,
        { method: "DELETE" },
      );
      setDeletingDocument(null);
      notify("Document deleted");
      documents.reload();
    } catch (err) {
      notify(err.message, "danger");
    } finally {
      setDocumentDeleteBusy(false);
    }
  }

  async function addAttribute(event) {
    event.preventDefault();
    setAttributeBusy(true);
    setAttributeError("");
    let parsedDefault;
    if (!array && defaultValue !== "") {
      if (kind === "integer") parsedDefault = Number.parseInt(defaultValue, 10);
      else if (kind === "float") parsedDefault = Number.parseFloat(defaultValue);
      else if (kind === "boolean") parsedDefault = defaultValue === "true";
      else parsedDefault = defaultValue;
      if (kind === "integer" && Number.isNaN(parsedDefault)) {
        setAttributeError("Default must be an integer");
        setAttributeBusy(false);
        return;
      }
      if (kind === "float" && Number.isNaN(parsedDefault)) {
        setAttributeError("Default must be a number");
        setAttributeBusy(false);
        return;
      }
    }
    try {
      await api(
        `/projects/${projectId}/databases/${databaseId}/collections/${collectionId}/attributes/${kind}`,
        {
          method: "POST",
          body: {
            key,
            required,
            array,
            size: size ? Number(size) : undefined,
            default: parsedDefault,
            elements:
              kind === "enum"
                ? elements
                    .split(",")
                    .map((value) => value.trim())
                    .filter(Boolean)
                : undefined,
          },
        },
      );
      setAttributeOpen(false);
      setKey("");
      setRequired(false);
      setArray(false);
      setSize("");
      setDefaultValue("");
      setElements("");
      collection.reload();
      notify("Attribute added");
    } catch (err) {
      setAttributeError(err.message);
    } finally {
      setAttributeBusy(false);
    }
  }

  async function removeAttribute() {
    setAttributeDeleteBusy(true);
    try {
      await api(
        `/projects/${projectId}/databases/${databaseId}/collections/${collectionId}/attributes/${removingAttribute.key}`,
        { method: "DELETE" },
      );
      setRemovingAttribute(null);
      collection.reload();
      notify("Attribute removed");
    } catch (err) {
      notify(err.message, "danger");
    } finally {
      setAttributeDeleteBusy(false);
    }
  }

  const textType = ["string", "email", "url", "ip", "enum", "datetime"].includes(kind);

  return (
    <>
      <div className="crumbs">
        <Link href="/projects">Projects</Link>
        <span className="sep">/</span>
        <Link href={`/projects/${projectId}`}>{projectId}</Link>
        <span className="sep">/</span>
        <Link href={`/projects/${projectId}/databases/${databaseId}`}>{databaseId}</Link>
        <span className="sep">/</span>
        <span>{collectionId}</span>
      </div>
      <PageHead
        eyebrow="Collection"
        title={data.name}
        description={`Table ow_${projectId}_${databaseId}.${collectionId} · ${attributes.length} attributes · ${total} document${total === 1 ? "" : "s"}`}
        actions={
          <button className="btn btn-primary" onClick={openCreateDocument}>
            <Icon name="plus" size={14} />
            New document
          </button>
        }
      />

      <div className="grid-2">
        <section className="panel">
          <div className="panel-head">
            <h2>Attributes</h2>
            <button className="btn btn-sm" onClick={() => setAttributeOpen(true)}>
              <Icon name="plus" size={13} />
              Add
            </button>
          </div>
          <div className="panel-body" style={{ paddingTop: 8, paddingBottom: 8 }}>
            {attributes.length === 0 ? (
              <Empty title="No attributes">
                Add attributes to validate and query document fields.
              </Empty>
            ) : (
              attributes.map((attribute) => (
                <div className="attribute" key={attribute.key}>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div className="attribute-key">{attribute.key}</div>
                    <div className="row" style={{ gap: 6, marginTop: 3 }}>
                      <span className="attribute-type">
                        {attribute.type}
                        {attribute.array ? "[]" : ""}
                      </span>
                      {attribute.required ? <Badge tone="warn">required</Badge> : null}
                      {attribute.size ? (
                        <span className="small subtle">max {attribute.size}</span>
                      ) : null}
                    </div>
                  </div>
                  <button
                    className="icon-button"
                    onClick={() => setRemovingAttribute(attribute)}
                    title="Remove attribute"
                  >
                    <Icon name="trash" size={14} />
                  </button>
                </div>
              ))
            )}
          </div>
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2>Access</h2>
            <Badge tone={data.documentSecurity ? "dark" : "muted"}>
              {data.documentSecurity ? "per-document" : "collection-wide"}
            </Badge>
          </div>
          <div className="panel-body">
            <p className="small subtle" style={{ marginTop: 0 }}>
              {data.documentSecurity
                ? "Reads are filtered by each document's own read permissions; creating still requires the collection create permission."
                : "Everyone allowed by the collection permissions can read every document in this collection."}
            </p>
            <div className="divider" />
            <div className="stack-tight">
              {(data.$permissions || []).length === 0 ? (
                <span className="small subtle">No collection permissions set.</span>
              ) : (
                data.$permissions.map((permission) => (
                  <span key={permission} className="mono" style={{ color: "var(--ink-soft)" }}>
                    {permission}
                  </span>
                ))
              )}
            </div>
          </div>
        </section>
      </div>

      <section className="panel" style={{ marginTop: 18 }}>
        <div className="panel-head">
          <h2>Documents</h2>
          <div className="btn-row">
            <button className="btn btn-sm" onClick={documents.reload}>
              <Icon name="refresh" size={13} />
              Refresh
            </button>
            <button className="btn btn-sm btn-primary" onClick={openCreateDocument}>
              <Icon name="plus" size={13} />
              New
            </button>
          </div>
        </div>

        {documents.error ? <Notice tone="danger">{documents.error.message}</Notice> : null}

        {documents.loading ? (
          <Loading />
        ) : rows.length === 0 ? (
          <Empty
            title="No documents"
            action={
              <button className="btn btn-primary btn-sm" onClick={openCreateDocument}>
                Create the first document
              </button>
            }
          >
            Documents are stored as JSONB in the collection table.
          </Empty>
        ) : (
          <>
            <div className="table-wrap">
              <table>
                <thead>
                  <tr>
                    <th>ID</th>
                    {columns.map((attribute) => (
                      <th key={attribute.key}>{attribute.key}</th>
                    ))}
                    <th>Updated</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {rows.map((document) => (
                    <tr key={document.$id}>
                      <td className="mono">{document.$id}</td>
                      {columns.map((attribute) => (
                        <td key={attribute.key}>{cell(document[attribute.key])}</td>
                      ))}
                      <td className="subtle">{formatDate(document.$updatedAt)}</td>
                      <td>
                        <div className="table-actions">
                          <button
                            className="btn btn-sm"
                            onClick={() => openEditDocument(document)}
                          >
                            Edit
                          </button>
                          <button
                            className="btn btn-sm btn-danger"
                            onClick={() => setDeletingDocument(document)}
                            aria-label="Delete document"
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

      {attributeOpen ? (
        <Modal
          title="Add attribute"
          subtitle="Existing documents are not rewritten; new values are validated on write."
          onClose={() => setAttributeOpen(false)}
          footer={
            <>
              <button className="btn" onClick={() => setAttributeOpen(false)}>
                Cancel
              </button>
              <button
                className="btn btn-primary"
                form="new-attribute"
                type="submit"
                disabled={attributeBusy}
              >
                {attributeBusy ? <Spinner dark /> : null}
                {attributeBusy ? "Adding…" : "Add attribute"}
              </button>
            </>
          }
        >
          <form id="new-attribute" className="form-grid" onSubmit={addAttribute}>
            <div className="form-row">
              <Field label="Type" htmlFor="attr-type">
                <select
                  id="attr-type"
                  className="select"
                  value={kind}
                  onChange={(event) => setKind(event.target.value)}
                >
                  {TYPES.map((type) => (
                    <option key={type} value={type}>
                      {type}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label="Key" htmlFor="attr-key">
                <input
                  id="attr-key"
                  className="input mono"
                  required
                  autoFocus
                  pattern="[A-Za-z][A-Za-z0-9_]{0,63}"
                  placeholder="title"
                  value={key}
                  onChange={(event) => setKey(event.target.value)}
                />
              </Field>
            </div>

            {kind === "enum" ? (
              <Field
                label="Allowed values"
                hint="Comma-separated. Required for enum attributes."
                htmlFor="attr-elements"
              >
                <input
                  id="attr-elements"
                  className="input"
                  placeholder="draft, published, archived"
                  value={elements}
                  onChange={(event) => setElements(event.target.value)}
                />
              </Field>
            ) : null}

            <div className="form-row">
              {textType ? (
                <Field label="Max size" hint="Characters for strings." htmlFor="attr-size">
                  <input
                    id="attr-size"
                    className="input"
                    type="number"
                    min={1}
                    placeholder="255"
                    value={size}
                    onChange={(event) => setSize(event.target.value)}
                  />
                </Field>
              ) : null}
              <Field label="Default" hint="Optional. Leave blank for null." htmlFor="attr-default">
                {kind === "boolean" ? (
                  <select
                    id="attr-default"
                    className="select"
                    value={defaultValue}
                    onChange={(event) => setDefaultValue(event.target.value)}
                  >
                    <option value="">No default</option>
                    <option value="true">true</option>
                    <option value="false">false</option>
                  </select>
                ) : (
                  <input
                    id="attr-default"
                    className="input"
                    value={defaultValue}
                    onChange={(event) => setDefaultValue(event.target.value)}
                  />
                )}
              </Field>
            </div>

            <div className="row" style={{ gap: 22 }}>
              <label className="check">
                <input
                  type="checkbox"
                  checked={required}
                  onChange={(event) => setRequired(event.target.checked)}
                />
                Required
              </label>
              <label className="check">
                <input
                  type="checkbox"
                  checked={array}
                  onChange={(event) => setArray(event.target.checked)}
                />
                Array
              </label>
            </div>

            {attributeError ? <Notice tone="danger">{attributeError}</Notice> : null}
          </form>
        </Modal>
      ) : null}

      {removingAttribute ? (
        <Modal
          title="Remove attribute?"
          subtitle={removingAttribute.key}
          onClose={() => setRemovingAttribute(null)}
          footer={
            <>
              <button className="btn" onClick={() => setRemovingAttribute(null)}>
                Cancel
              </button>
              <button
                className="btn btn-danger"
                onClick={removeAttribute}
                disabled={attributeDeleteBusy}
              >
                {attributeDeleteBusy ? <Spinner /> : <Icon name="trash" size={13} />}
                Remove attribute
              </button>
            </>
          }
        >
          <Notice tone="warn">
            The attribute definition is removed. Existing JSONB values remain in the table,
            but they are no longer validated or queryable through Appwrite queries.
          </Notice>
        </Modal>
      ) : null}

      {docOpen ? (
        <Modal
          title={editingDocument ? "Edit document" : "New document"}
          subtitle={editingDocument ? editingDocument.$id : data.$id}
          wide
          onClose={() => setDocOpen(false)}
          footer={
            <>
              <button className="btn" onClick={() => setDocOpen(false)}>
                Cancel
              </button>
              <button
                className="btn btn-primary"
                form="document-form"
                type="submit"
                disabled={documentBusy}
              >
                {documentBusy ? <Spinner dark /> : null}
                {documentBusy ? "Saving…" : editingDocument ? "Save document" : "Create document"}
              </button>
            </>
          }
        >
          <form id="document-form" className="form-grid" onSubmit={saveDocument}>
            {!editingDocument ? (
              <Field label="Document ID" hint="Optional. Leave blank to generate one." htmlFor="doc-id">
                <input
                  id="doc-id"
                  className="input mono"
                  placeholder="note-001"
                  value={documentId}
                  onChange={(event) => setDocumentId(event.target.value)}
                />
              </Field>
            ) : null}
            <Field
              label="Data (JSON)"
              hint="Only attributes defined on this collection are accepted."
              htmlFor="doc-json"
            >
              <textarea
                id="doc-json"
                className="textarea"
                rows={12}
                spellCheck={false}
                value={json}
                onChange={(event) => setJson(event.target.value)}
              />
            </Field>
            <Field
              label="Permissions"
              hint={'One per line, e.g. read("any"). Leave blank for default (current user).'}
              htmlFor="doc-permissions"
            >
              <textarea
                id="doc-permissions"
                className="textarea"
                rows={3}
                spellCheck={false}
                placeholder={'read("any")\nupdate("users")'}
                value={permissions}
                onChange={(event) => setPermissions(event.target.value)}
              />
            </Field>
            {documentError ? <Notice tone="danger">{documentError}</Notice> : null}
          </form>
        </Modal>
      ) : null}

      {deletingDocument ? (
        <Modal
          title="Delete document?"
          subtitle={`${data.name} · ${deletingDocument.$id}`}
          onClose={() => setDeletingDocument(null)}
          footer={
            <>
              <button className="btn" onClick={() => setDeletingDocument(null)}>
                Cancel
              </button>
              <button
                className="btn btn-danger"
                onClick={deleteDocument}
                disabled={documentDeleteBusy}
              >
                {documentDeleteBusy ? <Spinner /> : <Icon name="trash" size={13} />}
                Delete document
              </button>
            </>
          }
        >
          <Notice tone="danger">This permanently removes the document row.</Notice>
        </Modal>
      ) : null}

      <Toast message={toast?.message} tone={toast?.tone} />
    </>
  );
}
