"use client";

import { useRef, useState } from "react";

import { api } from "@/lib/api";
import { useToast } from "@/lib/hooks";
import { Icon, Modal, Notice, Spinner, Toast } from "./ui";

const ID_PATTERN = /^[A-Za-z0-9][A-Za-z0-9_-]{0,35}$/;
const KEY_PATTERN = /^[A-Za-z][A-Za-z0-9_]{0,63}$/;
const ATTRIBUTE_TYPES = [
  "string",
  "integer",
  "float",
  "boolean",
  "datetime",
  "email",
  "url",
  "ip",
  "enum",
];
const TEXT_TYPES = ["string", "email", "url", "ip", "enum", "datetime"];
const ACTIONS = ["read", "create", "update", "delete"];

const EXAMPLE = {
  name: "My app",
  id: "my-app",
  databases: [
    {
      name: "Main",
      id: "main",
      collections: [
        {
          name: "Notes",
          id: "notes",
          documentSecurity: false,
          permissions: { read: ["any"], create: ["users"] },
          attributes: [
            { key: "title", type: "string", required: true, size: 200 },
            { key: "done", type: "boolean", default: false },
            { key: "status", type: "enum", elements: ["draft", "published"] },
          ],
          documents: [
            { id: "note-1", data: { title: "Hello world", done: false, status: "published" } },
            { data: { title: "Second note", status: "draft" } },
          ],
        },
      ],
    },
  ],
};

function normalizePermissions(value, path, errors) {
  if (value === undefined || value === null) return undefined;
  const out = [];
  if (Array.isArray(value)) {
    for (const entry of value) {
      const match = typeof entry === "string" && entry.match(/^([a-z]+)\s*\(\s*"?([a-z]+)"?\s*\)$/);
      if (!match) {
        errors.push(`${path}: permission ${JSON.stringify(entry)} is not action("role")`);
        continue;
      }
      out.push(`${match[1]}("${match[2]}")`);
    }
    return out;
  }
  if (typeof value === "object") {
    for (const [action, roles] of Object.entries(value)) {
      if (!ACTIONS.includes(action)) {
        errors.push(`${path}: unknown permission action ${action}`);
        continue;
      }
      for (const role of Array.isArray(roles) ? roles : [roles]) {
        out.push(`${action}("${role}")`);
      }
    }
    return out;
  }
  errors.push(`${path}: permissions must be an object or an array`);
  return undefined;
}

function normalizeAttribute(raw, path, errors) {
  const attribute = {};
  const KNOWN_ATTRIBUTE_KEYS = ["key", "type", "required", "array", "size", "default", "elements"];
  const unknownAttributeKeys = Object.keys(raw).filter(
    (key) => !KNOWN_ATTRIBUTE_KEYS.includes(key),
  );
  if (unknownAttributeKeys.length > 0) {
    errors.push(
      `${path}: unknown key ${unknownAttributeKeys.map((key) => `"${key}"`).join(", ")}`,
    );
  }
  if (typeof raw.key !== "string" || !KEY_PATTERN.test(raw.key)) {
    errors.push(`${path}: key must match ${KEY_PATTERN.source}`);
  } else {
    attribute.key = raw.key;
  }
  if (!ATTRIBUTE_TYPES.includes(raw.type)) {
    errors.push(`${path}: type must be one of ${ATTRIBUTE_TYPES.join(", ")}`);
    return attribute;
  }
  attribute.type = raw.type;
  attribute.required = Boolean(raw.required);
  attribute.array = Boolean(raw.array);
  if (TEXT_TYPES.includes(raw.type) && raw.size !== undefined) {
    const size = Number(raw.size);
    if (!Number.isInteger(size) || size <= 0) errors.push(`${path}: size must be a positive integer`);
    else attribute.size = size;
  }
  if (raw.type === "enum") {
    const elements = Array.isArray(raw.elements) ? raw.elements.filter(Boolean) : [];
    if (elements.length === 0) errors.push(`${path}: enum attributes need a non-empty elements array`);
    else attribute.elements = elements.map(String);
  }
  if (raw.default !== undefined && raw.default !== null) {
    let value = raw.default;
    if ((raw.type === "integer" || raw.type === "float") && typeof value === "string") {
      value = raw.type === "integer" ? Number.parseInt(value, 10) : Number.parseFloat(value);
    }
    if ((raw.type === "integer" || raw.type === "float") && Number.isNaN(value)) {
      errors.push(`${path}: default is not a valid ${raw.type}`);
    } else {
      attribute.default = value;
    }
  }
  return attribute;
}

export function parseManifest(text) {
  const errors = [];
  let raw;
  try {
    raw = JSON.parse(text);
  } catch (err) {
    return { errors: [`Invalid JSON: ${err.message}`], manifest: null };
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    return { errors: ["The manifest must be a JSON object"], manifest: null };
  }

  const manifest = {};
  if (typeof raw.name !== "string" || !raw.name.trim()) {
    errors.push("name is required");
  } else {
    manifest.name = raw.name.trim();
  }
  if (raw.id !== undefined && raw.id !== null && raw.id !== "") {
    if (typeof raw.id !== "string" || !ID_PATTERN.test(raw.id.trim())) {
      errors.push(`id must match ${ID_PATTERN.source}`);
    } else {
      manifest.id = raw.id.trim();
    }
  }

  const databases = [];
  const KNOWN_ROOT_KEYS = ["name", "id", "databases", "collections"];
  const unknownRootKeys = Object.keys(raw).filter((key) => !KNOWN_ROOT_KEYS.includes(key));
  if (unknownRootKeys.length > 0) {
    errors.push(
      `unknown key ${unknownRootKeys.map((key) => `"${key}"`).join(", ")} at the top level`,
    );
  }
  if (raw.databases !== undefined && raw.collections !== undefined) {
    errors.push('set either "databases" or top-level "collections", not both');
  }
  let databaseSource = raw.databases;
  if (raw.databases === undefined && raw.collections !== undefined) {
    databaseSource = [{ name: "Main", id: "main", collections: raw.collections }];
  }
  const databaseIds = new Set();
  if (databaseSource !== undefined) {
    if (!Array.isArray(databaseSource)) {
      errors.push("databases must be an array");
    } else {
      databaseSource.forEach((database, index) => {
        const path = `databases[${index}]`;
        if (typeof database !== "object" || database === null) {
          errors.push(`${path}: must be an object`);
          return;
        }
        const KNOWN_DATABASE_KEYS = ["name", "id", "collections"];
        const unknownDatabaseKeys = Object.keys(database).filter(
          (key) => !KNOWN_DATABASE_KEYS.includes(key),
        );
        if (unknownDatabaseKeys.length > 0) {
          errors.push(
            `${path}: unknown key ${unknownDatabaseKeys.map((key) => `"${key}"`).join(", ")}`,
          );
        }
        const entry = { collections: [] };
        if (typeof database.name !== "string" || !database.name.trim()) {
          errors.push(`${path}: name is required`);
        } else {
          entry.name = database.name.trim();
        }
        if (database.id !== undefined && database.id !== null && database.id !== "") {
          if (typeof database.id !== "string" || !ID_PATTERN.test(database.id.trim())) {
            errors.push(`${path}: id must match ${ID_PATTERN.source}`);
          } else {
            entry.id = database.id.trim();
          }
        }
        if (entry.id) {
          if (databaseIds.has(entry.id)) {
            errors.push(`duplicate database id "${entry.id}"`);
          } else {
            databaseIds.add(entry.id);
          }
        }
        const collectionIds = new Set();
        if (database.collections !== undefined) {
          if (!Array.isArray(database.collections)) {
            errors.push(`${path}: collections must be an array`);
          } else {
            database.collections.forEach((collection, cIndex) => {
              const collectionPath = `${path}.collections[${cIndex}]`;
              if (typeof collection !== "object" || collection === null) {
                errors.push(`${collectionPath}: must be an object`);
                return;
              }
              const KNOWN_COLLECTION_KEYS = [
                "name",
                "id",
                "documentSecurity",
                "permissions",
                "attributes",
                "documents",
              ];
              const unknownCollectionKeys = Object.keys(collection).filter(
                (key) => !KNOWN_COLLECTION_KEYS.includes(key),
              );
              if (unknownCollectionKeys.length > 0) {
                errors.push(
                  `${collectionPath}: unknown key ${unknownCollectionKeys.map((key) => `"${key}"`).join(", ")}`,
                );
              }
              const collectionEntry = { attributes: [], documents: [] };
              if (typeof collection.name !== "string" || !collection.name.trim()) {
                errors.push(`${collectionPath}: name is required`);
              } else {
                collectionEntry.name = collection.name.trim();
              }
              if (collection.id !== undefined && collection.id !== null && collection.id !== "") {
                if (typeof collection.id !== "string" || !ID_PATTERN.test(collection.id.trim())) {
                  errors.push(`${collectionPath}: id must match ${ID_PATTERN.source}`);
                } else {
                  collectionEntry.id = collection.id.trim();
                }
              }
              if (collectionEntry.id) {
                if (collectionIds.has(collectionEntry.id)) {
                  errors.push(
                    `duplicate collection id "${collectionEntry.id}" in ${database.name || `database ${index}`}`,
                  );
                } else {
                  collectionIds.add(collectionEntry.id);
                }
              }
              collectionEntry.documentSecurity = Boolean(collection.documentSecurity);
              collectionEntry.permissions = normalizePermissions(
                collection.permissions,
                collectionPath,
                errors,
              );
              if (collection.attributes !== undefined) {
                if (!Array.isArray(collection.attributes)) {
                  errors.push(`${collectionPath}: attributes must be an array`);
                } else {
                  collection.attributes.forEach((attribute, aIndex) => {
                    collectionEntry.attributes.push(
                      normalizeAttribute(
                        typeof attribute === "object" && attribute !== null ? attribute : {},
                        `${collectionPath}.attributes[${aIndex}]`,
                        errors,
                      ),
                    );
                  });
                }
              }
              if (collection.documents !== undefined) {
                if (!Array.isArray(collection.documents)) {
                  errors.push(`${collectionPath}: documents must be an array`);
                } else {
                  const knownKeys = new Set(
                    collectionEntry.attributes.map((attribute) => attribute.key).filter(Boolean),
                  );
                  collection.documents.forEach((document, dIndex) => {
                    const documentPath = `${collectionPath}.documents[${dIndex}]`;
                    if (typeof document !== "object" || document === null || Array.isArray(document)) {
                      errors.push(`${documentPath}: must be an object`);
                      return;
                    }
                    const documentEntry = {};
                    const data =
                      typeof document.data === "object" &&
                      document.data !== null &&
                      !Array.isArray(document.data)
                        ? document.data
                        : null;
                    if (!data) {
                      errors.push(`${documentPath}: data must be an object`);
                    } else if (knownKeys.size === 0) {
                      errors.push(`${documentPath}: documents need attributes declared on the collection`);
                    } else {
                      const unknown = Object.keys(data).filter((key) => !knownKeys.has(key));
                      if (unknown.length > 0) {
                        errors.push(`${documentPath}: unknown attribute keys ${unknown.join(", ")}`);
                      }
                      documentEntry.data = data;
                    }
                    if (document.id !== undefined && document.id !== null && document.id !== "") {
                      if (typeof document.id !== "string" || !ID_PATTERN.test(document.id.trim())) {
                        errors.push(`${documentPath}: id must match ${ID_PATTERN.source}`);
                      } else {
                        documentEntry.id = document.id.trim();
                      }
                    }
                    documentEntry.permissions = normalizePermissions(
                      document.permissions,
                      documentPath,
                      errors,
                    );
                    collectionEntry.documents.push(documentEntry);
                  });
                }
              }
              entry.collections.push(collectionEntry);
            });
          }
        }
        databases.push(entry);
      });
    }
  }
  manifest.databases = databases;

  const counts = databases.reduce(
    (total, database) => ({
      collections: total.collections + database.collections.length,
      attributes:
        total.attributes +
        database.collections.reduce((sum, collection) => sum + collection.attributes.length, 0),
      documents:
        total.documents +
        database.collections.reduce(
          (sum, collection) => sum + (collection.documents?.length || 0),
          0,
        ),
    }),
    { collections: 0, attributes: 0, documents: 0 },
  );

  return { errors, manifest, counts };
}

function existsError(err) {
  return /exist|duplicate|already|unique|conflict/i.test(err.message || "");
}

function responseId(record) {
  return record?.$id || record?.id || null;
}

async function findProjectIdByName(name) {
  const data = await api("/projects");
  const match = (data.projects || []).find((entry) => entry.name === name);
  return match?.id || null;
}

async function findDatabaseId(projectId, name) {
  const data = await api(`/projects/${projectId}/databases`);
  const match = (data.databases || []).find((entry) => entry.name === name || entry.$id === name);
  return responseId(match);
}

async function findCollectionId(projectId, databaseId, name) {
  const data = await api(`/projects/${projectId}/databases/${databaseId}/collections`);
  const match = (data.collections || []).find(
    (entry) => entry.name === name || entry.$id === name,
  );
  return responseId(match);
}

export default function ProjectFromJson({ onClose, onCreated }) {
  const { toast, notify } = useToast();
  const fileInput = useRef(null);

  const [raw, setRaw] = useState("");
  const [fileName, setFileName] = useState(null);
  const [parsed, setParsed] = useState(null);
  const [running, setRunning] = useState(false);
  const [log, setLog] = useState([]);
  const [finished, setFinished] = useState(null);
  const [fatalError, setFatalError] = useState("");

  const errors = parsed?.errors || [];
  const manifest = parsed?.manifest || null;
  const canCreate = Boolean(manifest) && errors.length === 0 && !running;

  function update(text) {
    setRaw(text);
    setFileName(null);
    setLog([]);
    setFinished(null);
    setFatalError("");
    if (text.trim()) setParsed(parseManifest(text));
    else setParsed(null);
  }

  function onFile(event) {
    const file = event.target.files?.[0];
    if (!file) return;
    file
      .text()
      .then((text) => {
        setFileName(file.name);
        setRaw(text);
        setLog([]);
        setFinished(null);
        setFatalError("");
        setParsed(parseManifest(text));
      })
      .catch(() => setFatalError("Could not read the file"));
  }

  function step(status, label, detail) {
    setLog((current) => [...current, { status, label, detail }]);
  }

  async function create() {
    if (!canCreate) return;
    setRunning(true);
    setLog([]);
    setFatalError("");
    setFinished(null);
    let project = null;
    try {
      try {
        project = await api("/projects", {
          method: "POST",
          body: { name: manifest.name, id: manifest.id || undefined },
        });
        step("ok", "project", `${manifest.name}${manifest.id ? ` · ${manifest.id}` : ""}`);
      } catch (err) {
        if (existsError(err)) {
          const projectId =
            manifest.id || (await findProjectIdByName(manifest.name).catch(() => null));
          if (!projectId) {
            setFatalError("A project with this name already exists, but its ID could not be resolved");
            setRunning(false);
            return;
          }
          project = { id: projectId };
          step("exists", "project", `${manifest.name} · ${projectId}`);
        } else {
          setFatalError(err.message);
          setRunning(false);
          return;
        }
      }

      for (const database of manifest.databases) {
        const databaseLabel = `database ${database.name}${database.id ? ` (${database.id})` : ""}`;
        let databaseId = null;
        try {
          const created = await api(`/projects/${project.id}/databases`, {
            method: "POST",
            body: { name: database.name, id: database.id || undefined },
          });
          databaseId = responseId(created) || database.id;
          step("ok", databaseLabel);
        } catch (err) {
          if (existsError(err)) {
            databaseId =
              database.id ||
              (await findDatabaseId(project.id, database.name).catch(() => null));
            step("exists", databaseLabel);
          } else {
            step("fail", databaseLabel, err.message);
          }
        }
        if (!databaseId) {
          for (const collection of database.collections) {
            step("skip", `collection ${collection.name}`, "database failed");
          }
          continue;
        }

        for (const collection of database.collections) {
          const collectionLabel = `collection ${databaseId}/${collection.id || collection.name}`;
          let collectionId = null;
          try {
            const created = await api(
              `/projects/${project.id}/databases/${databaseId}/collections`,
              {
                method: "POST",
                body: {
                  name: collection.name,
                  id: collection.id || undefined,
                  documentSecurity: collection.documentSecurity,
                  permissions: collection.permissions,
                },
              },
            );
            collectionId = responseId(created) || collection.id;
            step("ok", collectionLabel);
          } catch (err) {
            if (existsError(err)) {
              collectionId =
                collection.id ||
                (await findCollectionId(project.id, databaseId, collection.name).catch(() => null));
              step("exists", collectionLabel);
            } else {
              step("fail", collectionLabel, err.message);
            }
          }
          if (!collectionId) {
            for (const attribute of collection.attributes) {
              step("skip", `attribute ${collection.name}/${attribute.key}`, "collection failed");
            }
            continue;
          }

          for (const attribute of collection.attributes) {
            const attributeLabel = `attribute ${databaseId}/${collectionId}/${attribute.key}`;
            try {
              await api(
                `/projects/${project.id}/databases/${databaseId}/collections/${collectionId}/attributes/${attribute.type}`,
                {
                  method: "POST",
                  body: {
                    key: attribute.key,
                    required: attribute.required,
                    array: attribute.array,
                    size: attribute.size,
                    default: attribute.default,
                    elements: attribute.elements,
                  },
                },
              );
              step("ok", attributeLabel);
            } catch (err) {
              if (existsError(err)) {
                step("exists", attributeLabel);
              } else {
                step("fail", attributeLabel, err.message);
              }
            }
          }

          for (const document of collection.documents || []) {
            const documentLabel = `document ${databaseId}/${collectionId}${document.id ? `/${document.id}` : ""}`;
            try {
              await api(
                `/projects/${project.id}/databases/${databaseId}/collections/${collectionId}/documents`,
                {
                  method: "POST",
                  body: {
                    id: document.id || undefined,
                    data: document.data,
                    permissions: document.permissions,
                  },
                },
              );
              step("ok", documentLabel);
            } catch (err) {
              if (existsError(err)) {
                step("exists", documentLabel);
              } else {
                step("fail", documentLabel, err.message);
              }
            }
          }
        }
      }
      setFinished(project);
      notify("Project created from JSON");
      onCreated?.(project);
    } finally {
      setRunning(false);
    }
  }

  const counts = parsed?.counts;

  return (
    <>
      <Modal
      title="Create project from JSON"
      subtitle="One file lays out the project: databases, collections, attributes, permissions and documents"
      wide
      onClose={onClose}
      footer={
        <>
          {finished ? (
            <a className="btn btn-primary" href={`/projects/${finished.id}`}>
              Open project <Icon name="arrow-right" size={13} />
            </a>
          ) : (
            <button className="btn btn-primary" onClick={create} disabled={!canCreate}>
              {running ? <Spinner dark /> : null}
              {running ? "Creating…" : "Create project"}
            </button>
          )}
          <button className="btn" onClick={onClose} disabled={running}>
            {finished ? "Close" : "Cancel"}
          </button>
        </>
      }
    >
      <div className="form-grid">
        {!running && !finished ? (
          <>
            <label className="file-drop">
              <input
                ref={fileInput}
                type="file"
                accept=".json,application/json"
                onChange={onFile}
              />
              <Icon name="import" size={16} />
              <span>
                Choose a .json file
                <span className="file-drop-sub">or paste the manifest below</span>
              </span>
              {fileName ? <span className="mono">{fileName}</span> : null}
            </label>

            <textarea
              className="textarea"
              rows={10}
              spellCheck={false}
              placeholder={`{\n  "name": "My app",\n  "id": "my-app",\n  "databases": [ … ]\n}`}
              value={raw}
              onChange={(event) => update(event.target.value)}
            />

            <div className="row">
              <button className="btn btn-sm btn-ghost" onClick={() => update(JSON.stringify(EXAMPLE, null, 2))}>
                <Icon name="code" size={13} />
                Load example
              </button>
              <span className="hint">
                Attributes: key, type (string, integer, float, boolean, datetime, email, url, ip, enum), plus size, elements, default, required, array. Collections accept documents with data and permissions. A bare collections array at the top level goes into one database named Main.
              </span>
            </div>

            {errors.length > 0 ? (
              <Notice tone="danger">
                <div className="stack-tight mono">
                  {errors.map((error) => (
                    <span key={error}>{error}</span>
                  ))}
                </div>
              </Notice>
            ) : null}

            {manifest && errors.length === 0 ? (
              <Notice tone="ok">
                <div className="stack-tight">
                  <span>
                    {manifest.name}
                    {manifest.id ? ` · ${manifest.id}` : ""} · {manifest.databases.length}{" "}
                    {manifest.databases.length === 1 ? "database" : "databases"} ·{" "}
                    {counts.collections} {counts.collections === 1 ? "collection" : "collections"} ·{" "}
                    {counts.attributes} {counts.attributes === 1 ? "attribute" : "attributes"}
                    {counts.documents > 0
                      ? ` · ${counts.documents} ${counts.documents === 1 ? "document" : "documents"}`
                      : ""}
                  </span>
                  {manifest.databases.map((database) => (
                    <span key={database.name} className="mono">
                      {database.name} — {database.collections.length}{" "}
                      {database.collections.length === 1 ? "collection" : "collections"}
                    </span>
                  ))}
                </div>
              </Notice>
            ) : null}
          </>
        ) : null}

        {log.length > 0 ? (
          <div className="log" role="log">
            {log.map((entry, index) => (
              <div className={`log-row ${entry.status}`} key={index}>
                <span className="log-status">{entry.status}</span>
                <span className="log-path">{entry.label}</span>
                {entry.detail ? <span className="log-detail">— {entry.detail}</span> : null}
              </div>
            ))}
          </div>
        ) : null}

        {fatalError ? <Notice tone="danger">{fatalError}</Notice> : null}
      </div>
      </Modal>
      <Toast message={toast?.message} tone={toast?.tone} />
    </>
  );
}
