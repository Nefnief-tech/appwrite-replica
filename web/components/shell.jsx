"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useEffect, useState } from "react";

import { api, initials } from "@/lib/api";
import { Icon, Spinner } from "./ui";

const NAV = [
  { href: "/", label: "Overview", icon: "overview" },
  { href: "/projects", label: "Projects", icon: "projects" },
  { href: "/usage", label: "Usage", icon: "usage" },
  { href: "/nodes", label: "Nodes", icon: "nodes" },
  { href: "/mail", label: "Mail", icon: "mail" },
  { href: "/import", label: "Import", icon: "import" },
  { href: "/design", label: "Design", icon: "code" },
];

function active(pathname, href) {
  return href === "/" ? pathname === "/" : pathname.startsWith(href);
}

export default function Shell({ children }) {
  const pathname = usePathname();
  const router = useRouter();
  const [me, setMe] = useState(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    api("/me")
      .then(setMe)
      .catch(() => setFailed(true));
  }, []);

  useEffect(() => {
    if (failed) router.replace("/login");
  }, [failed, router]);

  if (!me) {
    return (
      <div className="boot">
        <Spinner />
      </div>
    );
  }

  async function logout() {
    await api("/logout", { method: "POST" }).catch(() => {});
    window.location.assign("/login");
  }

  return (
    <div className="shell">
      <aside className="sidebar">
        <Link href="/" className="brand" aria-label="Openwrite home">
          <span className="brand-mark">O</span>
          <span className="brand-name">
            Open<em>write</em>
          </span>
        </Link>

        <nav className="nav-group" aria-label="Main">
          <span className="nav-label">Workspace</span>
          {NAV.map((item) => (
            <Link
              key={item.href}
              href={item.href}
              className={`nav-link ${active(pathname, item.href) ? "active" : ""}`}
            >
              <Icon name={item.icon} size={16} />
              {item.label}
            </Link>
          ))}
        </nav>

        <div className="sidebar-foot">
          <span className="avatar">{initials(me.name, me.email)}</span>
          <div className="sidebar-user">
            <strong>{me.name}</strong>
            <span>{me.email}</span>
          </div>
          <button className="icon-button" onClick={logout} title="Sign out" aria-label="Sign out">
            <Icon name="logout" size={16} />
          </button>
        </div>
      </aside>

      <main className="content">
        <div className="content-inner">{children}</div>
      </main>
    </div>
  );
}
