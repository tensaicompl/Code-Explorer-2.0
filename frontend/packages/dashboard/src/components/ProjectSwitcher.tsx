import { useEffect, useRef, useState } from "react";
import { ConfirmRebuild } from "./SourcePanel";

const API_BASE = import.meta.env.VITE_API_BASE ?? "";

interface Props {
  /** Currently loaded project+stream, e.g. "myproject/main". */
  current: string;
  token: string | null;
}

/**
 * Header control for switching the loaded project — / P6.
 *
 * Why this exists: `resolveProjectStream()` (App.tsx:99) resolves
 * `?project=` → `localStorage` → a placeholder name, and *writes the result
 * back to localStorage*. So the first project ever opened became sticky, and
 * loading the app with no query string kept serving it forever with no way to
 * change it from the UI. The header showed the truth — it reads the loaded
 * graph's own metadata — which made it look like a bug in the graph rather than
 * a stale preference.
 *
 * `/api/graph/projects` already lists every built graph, so this needs no new
 * backend surface.
 */
export default function ProjectSwitcher({ current, token }: Props) {
  const [available, setAvailable] = useState<string[]>([]);
  const [open, setOpen] = useState(false);
  const [adding, setAdding] = useState(false);
  const [newName, setNewName] = useState("");
  const [newPath, setNewPath] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  /**
   * Close on an outside click via a document listener rather than a full-screen
   * catcher div.
   *
   * The catcher was the first attempt and it silently broke the control: a
   * `fixed inset-0` sibling sat over the menu and swallowed every click on an
   * option, so the dropdown opened and then refused to select anything. Raising
   * the menu's z-index does not reliably fix it either — the menu is `absolute`
   * inside a positioned ancestor, so its z-index is scoped to that ancestor's
   * stacking context while the `fixed` catcher escapes to the root.
   */
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as globalThis.Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  useEffect(() => {
    if (!token) return;
    fetch(`${API_BASE}/api/graph/projects`, {
      headers: { Authorization: `Bearer ${token}` },
    })
      .then((r) => (r.ok ? r.json() : null))
      .then((b) => {
        const graphs: string[] = b?.graphs ?? [];
        setAvailable(graphs);

        // Landing on a real graph is App.tsx's job: it must happen even when
        // no graph loaded, and this component does not render until one has.
      })
      .catch(() => setAvailable([]));
  }, [token]);

  /**
   * Registry keys are `<project>_<stream>`, and both halves are sanitised to
   * `[a-z0-9_]` by `registry_key()`. Splitting on the LAST underscore is
   * therefore the correct inverse: a project named `my_app` on stream
   * `develop` is `my_app_develop`, and splitting on the first would ask the
   * backend for project `my`.
   */
  const parse = (key: string): { project: string; stream: string } => {
    const i = key.lastIndexOf("_");
    return i < 0
      ? { project: key, stream: "develop" }
      : { project: key.slice(0, i), stream: key.slice(i + 1) };
  };

  /**
   * Create the project and immediately switch to it.
   *
   * The new project lands greyed out and empty — its index does not exist yet —
   * which is the same locked state a source change produces. One state, one
   * explanation, rather than a separate "pending" concept for new projects.
   */
  const addProject = async () => {
    setBusy(true);
    setError(null);
    const project = newName.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-");
    try {
      const res = await fetch(`${API_BASE}/api/graph/source`, {
        method: "PUT",
        headers: {
          "Content-Type": "application/json",
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
        },
        body: JSON.stringify({ project, stream: "develop", path: newPath.trim() }),
      });
      const body = await res.json().catch(() => null);
      if (!res.ok) {
        setError(body?.detail ?? `Request failed (${res.status}).`);
        return;
      }
      const params = new URLSearchParams(window.location.search);
      params.set("project", project);
      params.set("stream", "develop");
      window.location.search = params.toString();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const switchTo = (key: string) => {
    const { project, stream } = parse(key);
    // Navigate rather than mutate state: the whole store is keyed off the
    // loaded graph, and a full load is both simpler and less surprising than
    // trying to swap a graph out underneath an open layer.
    const params = new URLSearchParams(window.location.search);
    params.set("project", project);
    params.set("stream", stream);
    window.location.search = params.toString();
  };

  // No early return for "one project or none": collapsing to plain text also
  // removed the only way to ADD one, which is a dead end on a fresh install.
  // The menu always renders.

  return (
    <div className="relative hidden lg:block" ref={rootRef}>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-haspopup="listbox"
        aria-expanded={open}
        className="flex items-center gap-2 text-sm text-text-muted hover:text-text-primary transition-colors truncate max-w-[240px]"
        title={`${current} — switch project`}
      >
        <span className="truncate">{current}</span>
        <span style={{ fontSize: 9 }} aria-hidden="true">
          ▾
        </span>
      </button>

      {open && (
        <ul
            role="listbox"
            className="absolute left-0 top-full mt-2 z-50 min-w-[220px] bg-surface border border-border-subtle bevel-sm bevel-elevate py-1"
          >
            {available.map((key) => {
              const { project, stream } = parse(key);
              const label = `${project}/${stream}`;
              const isCurrent = label === current;
              return (
                <li key={key} role="option" aria-selected={isCurrent}>
                  <button
                    type="button"
                    onClick={() => switchTo(key)}
                    className={`w-full text-left px-4 py-2 text-sm transition-colors ${
                      isCurrent
                        ? "text-accent bg-accent/10"
                        : "text-text-secondary hover:text-text-primary hover:bg-elevated"
                    }`}
                  >
                    {label}
                  </button>
                </li>
              );
            })}
          <li className="border-t border-border-subtle mt-1 pt-1">
            <button
              type="button"
              onClick={() => { setOpen(false); setAdding(true); setError(null); }}
              className="w-full text-left px-4 py-2 text-sm text-text-muted hover:text-accent transition-colors"
            >
              + Add project…
            </button>
          </li>
        </ul>
      )}

      {adding && (
        <div
          className="fixed inset-0 z-[100] flex items-center justify-center bg-root/80 px-4"
          role="dialog"
          aria-modal="true"
          aria-label="Add project"
        >
          <div className="w-full max-w-lg bg-surface border border-border-medium bevel bevel-elevate p-6">
            <h2 className="prx-heading text-lg text-text-primary mb-1">Add a project</h2>
            <p className="text-sm text-text-muted mb-5">
              Point Insight at a folder on this machine or at a Git repository.
            </p>

            <label className="block text-[10px] uppercase tracking-[0.1em] text-text-muted mb-1">
              Project name
            </label>
            <input
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              placeholder="my-service"
              autoFocus
              className="w-full mb-4 px-3 py-2 bg-elevated border border-border-subtle bevel-sm text-text-primary text-sm focus:outline-none focus:border-accent"
            />

            <label className="block text-[10px] uppercase tracking-[0.1em] text-text-muted mb-1">
              Local path or repository URL
            </label>
            <input
              value={newPath}
              onChange={(e) => setNewPath(e.target.value)}
              placeholder="/path/to/repo  ·  https://github.com/org/repo.git"
              className="w-full mb-2 px-3 py-2 bg-elevated border border-border-subtle bevel-sm text-text-primary text-xs font-mono focus:outline-none focus:border-accent"
            />
            <p className="text-[11px] text-text-muted mb-5">
              GitHub, GitLab, Bitbucket and any other Git host are accepted over
              HTTPS or SSH.
            </p>

            {error && <p className="text-sm text-red-400 mb-4 break-words">{error}</p>}

            <div className="flex gap-3 justify-end">
              <button
                type="button"
                onClick={() => { setAdding(false); setError(null); }}
                className="px-4 py-2 text-sm bevel-sm border border-border-subtle text-text-muted hover:text-text-primary transition-colors"
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={!newName.trim() || !newPath.trim()}
                onClick={() => { setAdding(false); setConfirming(true); }}
                className="px-4 py-2 text-sm bg-cta text-cta-fg font-medium uppercase tracking-[0.04em] bevel-sm hover:bg-cta-hover transition-colors disabled:opacity-40"
              >
                Continue
              </button>
            </div>
          </div>
        </div>
      )}

      {confirming && (
        <ConfirmRebuild
          title={`Add "${newName.trim()}"?`}
          path={newPath.trim()}
          kind={/^(https?:\/\/|git@|ssh:\/\/)/i.test(newPath.trim()) ? "git" : "local"}
          busy={busy}
          error={error}
          onCancel={() => { setConfirming(false); setError(null); }}
          onConfirm={() => void addProject()}
        />
      )}
    </div>
  );
}
