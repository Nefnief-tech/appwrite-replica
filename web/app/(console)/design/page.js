import { Badge, Icon, Notice, PageHead } from "@/components/ui";

const COLORS = [
  ["bg", "Canvas"], ["sunken", "Sunken"], ["panel", "Panel"],
  ["panel-raised", "Raised"], ["panel-overlay", "Overlay"], ["line", "Border"],
  ["line-strong", "Border strong"], ["ink", "Text"], ["ink-soft", "Text soft"],
  ["muted", "Muted"], ["accent", "Accent"], ["ok", "Success"],
  ["warn", "Warning"], ["danger", "Danger"],
];

const TYPES = [
  ["34px / 600", "Page title", "h1"],
  ["18px / 600", "Section title", "h2"],
  ["14px / 400", "Body", "p"],
  ["13px / 500", "Control / table", "span"],
  ["12px / 400", "Small", "small"],
  ["11px / 500", "Mono label", "code"],
];

export default function DesignPage() {
  return (
    <>
      <PageHead
        eyebrow="Reference"
        title="Design system"
        description="Live tokens and primitives for the dark console. Every value below comes from globals.css."
      />

      <div className="stack">
        <section className="panel">
          <div className="panel-head"><h2>Color</h2><span className="mono">14 tokens</span></div>
          <div className="panel-body">
            <div className="grid-4">
              {COLORS.map(([token, label]) => (
                <div className="row" key={token}>
                  <span style={{ width: 28, height: 28, borderRadius: 6, background: `var(--${token})`, border: "1px solid var(--line)" }} />
                  <div>
                    <div className="mono" style={{ color: "var(--ink)" }}>{token}</div>
                    <div className="small subtle">{label}</div>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </section>

        <section className="panel">
          <div className="panel-head"><h2>Type</h2><span className="mono">Inter / JetBrains Mono</span></div>
          <div className="panel-body stack-tight">
            {TYPES.map(([size, label, tag]) => (
              <div className="spread" key={label}>
                <span style={{ fontSize: size.split(" ")[0] }}>{label}</span>
                <span className="mono subtle">{size} · {tag}</span>
              </div>
            ))}
          </div>
        </section>

        <section className="panel">
          <div className="panel-head"><h2>Buttons</h2></div>
          <div className="panel-body btn-row">
            <button className="btn btn-primary">Primary</button>
            <button className="btn">Secondary</button>
            <button className="btn btn-dark">Inverted</button>
            <button className="btn btn-ghost">Ghost</button>
            <button className="btn btn-danger">Danger</button>
            <button className="btn btn-sm">Small</button>
            <button className="btn" disabled>Disabled</button>
          </div>
        </section>

        <section className="panel">
          <div className="panel-head"><h2>Form</h2></div>
          <div className="panel-body form-grid">
            <div className="form-row">
              <div className="field">
                <label htmlFor="d-name">Text field</label>
                <input id="d-name" className="input" placeholder="placeholder" />
                <div className="hint">Helper text in muted tone.</div>
              </div>
              <div className="field">
                <label htmlFor="d-select">Select</label>
                <select id="d-select" className="select"><option>Option one</option><option>Option two</option></select>
              </div>
            </div>
            <label className="check"><input type="checkbox" defaultChecked /> Checkbox</label>
            <div className="row">
              <button className="toggle on" type="button" aria-label="Toggle on" />
              <button className="toggle" type="button" aria-label="Toggle off" />
            </div>
          </div>
        </section>

        <section className="panel">
          <div className="panel-head"><h2>Feedback</h2></div>
          <div className="panel-body stack-tight">
            <Notice>Informational notice with the default surface.</Notice>
            <Notice tone="ok">Success notice.</Notice>
            <Notice tone="warn">Warning notice.</Notice>
            <Notice tone="danger">Danger notice.</Notice>
            <div className="btn-row">
              <Badge>Default</Badge><Badge tone="ok">Healthy</Badge><Badge tone="warn">Degraded</Badge>
              <Badge tone="danger">Down</Badge><Badge tone="acid">Accent</Badge>
            </div>
          </div>
        </section>

        <section className="panel">
          <div className="panel-head"><h2>Table</h2><span className="mono">Tabular numerals</span></div>
          <div className="table-wrap">
            <table>
              <thead><tr><th>Name</th><th>ID</th><th>Status</th><th>Rows</th></tr></thead>
              <tbody>
                <tr><td className="strong">semester<span className="cell-sub">Main dataset</span></td><td className="mono">k1mCw25j</td><td><Badge tone="ok">Active</Badge></td><td>1,204</td></tr>
                <tr><td className="strong">archive</td><td className="mono">dZ907x2k</td><td><Badge tone="muted">Idle</Badge></td><td>87</td></tr>
              </tbody>
            </table>
          </div>
        </section>

        <section className="panel">
          <div className="panel-head"><h2>Navigation</h2></div>
          <div className="panel-body">
            <div className="tabs"><button className="tab active">Overview</button><button className="tab">Settings</button></div>
            <div className="crumbs">Console <span className="sep">/</span> Design system</div>
            <Icon name="database" size={18} />
          </div>
        </section>
      </div>
    </>
  );
}
