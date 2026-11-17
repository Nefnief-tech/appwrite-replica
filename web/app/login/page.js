"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";

import { api } from "@/lib/api";
import { Field, Icon, Spinner } from "@/components/ui";

export default function LoginPage() {
  const router = useRouter();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit(event) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      await api("/login", { method: "POST", body: { email, password } });
      router.replace("/");
    } catch (err) {
      setError(err.message);
      setBusy(false);
    }
  }

  return (
    <div className="login">
      <section className="login-art">
        <div className="login-brand">
          <span className="brand-mark">O</span>
          <span className="brand-name">
            Open<em>write</em>
          </span>
        </div>

        <div className="login-copy">
          <p className="eyebrow" style={{ color: "var(--accent)" }}>
            Self-hosted · Appwrite compatible
          </p>
          <h1>
            Your backend.
            <br />
            Your <em>Postgres.</em>
          </h1>
          <p>
            Projects, users, collections and documents — served by a Rust API on
            infrastructure you actually own. Bring a leader and as many replicas as
            you like.
          </p>
        </div>

        <div className="login-meta">
          <span>Postgres + Redis</span>
          <span>Appwrite API v1</span>
          <span>Admin console</span>
        </div>
      </section>

      <section className="login-panel">
        <form className="login-form" onSubmit={submit}>
          <p className="eyebrow">Admin access</p>
          <h2>Sign in</h2>
          <p className="subtle">Use the admin credentials configured on the API</p>

          <Field label="Email" htmlFor="email">
            <input
              id="email"
              className="input"
              type="email"
              autoComplete="username"
              required
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              placeholder="admin@example.com"
            />
          </Field>
          <Field label="Password" htmlFor="password">
            <input
              id="password"
              className="input"
              type="password"
              autoComplete="current-password"
              required
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              placeholder="••••••••••"
            />
          </Field>

          {error ? (
            <div className="notice danger" style={{ marginTop: 12 }}>
              <Icon name="warning" size={15} />
              <div>{error}</div>
            </div>
          ) : null}

          <button className="btn btn-primary" type="submit" disabled={busy}>
            {busy ? <Spinner dark /> : <Icon name="arrow-right" size={15} />}
            {busy ? "Signing in…" : "Enter console"}
          </button>

          <div className="login-foot">Openwrite · Admin console</div>
        </form>
      </section>
    </div>
  );
}
