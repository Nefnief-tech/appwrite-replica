# Dark design system — Openwrite console

Date: 2026-11-17. Status: approved.

## Goal

Replace the warm cream/editorial console theme with a dark, crisp dev-tool theme. Establish a real
token layer so future UI work edits tokens, not 46 scattered hex values.

## Decisions

- **Dark only.** No toggle, no duplicated token sets.
- **Crisp dev-tool.** Neutral near-black layers, 1px borders, no serif. Inter for UI, JetBrains Mono
  for IDs/keys/metrics.
- **Keep terracotta as the single accent** (`#e0653f`) — the only surviving brand marker.
- **CSS classes stay the primitive API.** Pages already use `.btn`, `.input`, `.badge`, `.panel`,
  `.table-*`. React wrappers would change zero pixels and touch every page. Add one `/design` page
  that renders tokens/primitives live as the reference.

## Tokens

Layers: `--bg #0a0b0d`, `--panel #121316`, `--panel-raised #181a1e`, `--panel-overlay #1f2227`,
`--sunken #0d0e11`.
Borders: `--line #24262b`, `--line-strong #34373d`.
Text: `--ink #f1f2f4`, `--ink-soft #b0b5bd`, `--muted #767d88`.
Accent: `--accent #e0653f`, `--accent-deep #c9532e`, `--accent-soft rgba(224,101,63,.12)`,
`--focus rgba(224,101,63,.35)`.
Semantic: `--ok #3fb950`, `--warn #d29922`, `--danger #f85149` + soft variants.
Type: 11/12/13/14/22/28/40, mono tabular for numbers.
Radii: 4/6/10. Heights: btn 30, input 32.
Shadow: only modal/overlay/toast.

## Scope

- `web/app/globals.css` — token layer + all selectors restyled; remove all hardcoded hex/rgba.
- `web/app/layout.js` — drop Instrument Serif font.
- `web/app/design/page.js` — new live token/primitive reference page.
- `web/components/charts.jsx` — already uses CSS vars, no change expected.
- No backend, no page logic changes.

## Verification

- `npm run build` passes.
- Serve and inspect Overview, Login, a project's Databases page, and `/design` in the browser.
- Contrast: body text and input text must be readable on all three panel layers.
