import { useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";

const API_BASE = import.meta.env.VITE_API_BASE ?? "";

export interface ProjectSource {
  path: string;
  kind: "local" | "git";
  status: "ready" | "indexing" | "failed";
  message: string;
  recorded?: boolean;
}

/**
 * Poll a project's source while it is being rebuilt.
 *
 * Polling rather than a socket because a rebuild is minutes long and rare; a
 * 3-second poll costs nothing and cannot get stuck half-open. It stops as soon
 * as the status settles, so an idle dashboard makes no requests at all.
 */
export function useProjectSource(project: string, stream: string, token: string | null) {
  const [source, setSource] = useState<ProjectSource | null>(null);

  const refresh = useCallback(async () => {
    if (!token || !project) return null;
    try {
      const res = await fetch(
        `${API_BASE}/api/graph/source?project=${encodeURIComponent(project)}&stream=${encodeURIComponent(stream)}`,
        { headers: { Authorization: `Bearer ${token}` } },
      );
      if (!res.ok) return null;
      const body = (await res.json()) as ProjectSource;
      setSource(body);
      return body;
    } catch {
      return null;
    }
  }, [project, stream, token]);

  useEffect(() => { void refresh(); }, [refresh]);

  useEffect(() => {
    if (source?.status !== "indexing") return;
    const id = setInterval(() => { void refresh(); }, 3000);
    return () => clearInterval(id);
  }, [source?.status, refresh]);

  return { source, refresh, setSource };
}

interface ConfirmProps {
  title: string;
  path: string;
  kind: "local" | "git";
  busy: boolean;
  error: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}

/**
 * The consequences dialog.
 *
 * Changing where a project's code comes from invalidates every chunk, symbol,
 * call and graph node derived from the old location, so this is not a
 * preference — it discards the index and rebuilds it. Saying so plainly, with
 * the two real consequences (a full re-index, and the project being unusable
 * meanwhile), is the difference between an informed click and a surprise.
 */
export function ConfirmRebuild({
  title, path, kind, busy, error, onCancel, onConfirm,
}: ConfirmProps) {
  /**
   * Rendered into document.body, not in place.
   *
   * `position: fixed` is only relative to the viewport while no ancestor has a
   * transform, filter or backdrop-filter — any of those makes the ancestor the
   * containing block instead. This dialog is opened from the project switcher
   * and the source panel, both of which sit inside bevelled, drop-shadowed and
   * blurred chrome, so "fixed inset-0" was centring it inside a dropdown a few
   * hundred pixels wide rather than over the application. A portal takes it out
   * of that subtree entirely, so it cannot be recaptured by styling anywhere
   * above it.
   */
  return createPortal(
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-root/80 px-4"
      role="dialog"
      aria-modal="true"
      aria-label={title}
    >
      <div className="w-full max-w-lg bg-surface border border-border-medium bevel bevel-elevate p-6">
        <h2 className="prx-heading text-lg text-text-primary mb-3">{title}</h2>

        <p className="text-sm text-text-secondary leading-relaxed mb-3">
          Insight will re-index this source from scratch and rebuild its graph.
        </p>

        <div className="bevel-sm border border-border-subtle bg-root/40 px-3 py-2 mb-4">
          <div className="text-[10px] uppercase tracking-[0.1em] text-text-muted mb-1">
            {kind === "git" ? "Repository" : "Local path"}
          </div>
          <div className="font-mono text-xs text-text-primary break-all">{path}</div>
        </div>

        <ul className="text-sm text-text-secondary space-y-1.5 mb-5 list-disc list-inside">
          <li>The existing index and graph for this project are discarded.</li>
          <li>
            The project stays <strong>locked and unavailable</strong> until the
            rebuild finishes.
          </li>
          <li>This can take several minutes on a large codebase.</li>
        </ul>

        {error && (
          <p className="text-sm text-red-400 mb-4 break-words">{error}</p>
        )}

        <div className="flex gap-3 justify-end">
          <button
            type="button"
            onClick={onCancel}
            disabled={busy}
            className="px-4 py-2 text-sm bevel-sm border border-border-subtle text-text-muted hover:text-text-primary transition-colors disabled:opacity-40"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={busy}
            className="px-4 py-2 text-sm bg-cta text-cta-fg font-medium uppercase tracking-[0.04em] bevel-sm hover:bg-cta-hover transition-colors disabled:opacity-40"
          >
            {busy ? "Starting…" : "Re-index and rebuild"}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

/** Status strip shown while a project is locked, or after a failure. */
export function SourceStatus({ source }: { source: ProjectSource }) {
  if (source.status === "ready") return null;
  const failed = source.status === "failed";
  return (
    <div
      className={`mt-2 px-3 py-2 bevel-sm border text-xs ${
        failed
          ? "border-red-500/40 bg-red-500/10 text-red-300"
          : "border-accent/40 bg-accent/10 text-accent"
      }`}
      role="status"
    >
      {failed ? (
        <>
          <strong>Rebuild failed.</strong> {source.message || "See the backend log."}
        </>
      ) : (
        <>
          <strong>Locked — rebuilding.</strong>{" "}
          {source.message || "Indexing…"} This project is unavailable until it finishes.
        </>
      )}
    </div>
  );
}

/**
 * Editable source row for the Project Overview panel.
 *
 * Deliberately placed above LANGUAGES: everything below it is derived FROM this
 * path, so showing the derived facts before their origin reads backwards.
 */
export function SourceRow({
  project, stream, token, onLockedChange,
}: {
  project: string;
  stream: string;
  token: string | null;
  onLockedChange?: (locked: boolean) => void;
}) {
  const { source, refresh, setSource } = useProjectSource(project, stream, token);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  // What the confirm dialog is about to do. Re-pointing and re-indexing hit the
  // same endpoint with the same consequences, so they share one dialog and
  // differ only in the path they submit and the question they ask.
  const [pending, setPending] = useState<{ path: string; mode: "repoint" | "reindex" } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    onLockedChange?.(source?.status === "indexing");
  }, [source?.status, onLockedChange]);

  if (!source) return null;
  const locked = source.status === "indexing";

  const submit = async (path: string) => {
    setBusy(true);
    setError(null);
    try {
      const res = await fetch(`${API_BASE}/api/graph/source`, {
        method: "PUT",
        headers: {
          "Content-Type": "application/json",
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
        },
        body: JSON.stringify({ project, stream, path }),
      });
      const body = await res.json().catch(() => null);
      if (!res.ok) {
        setError(body?.detail ?? `Request failed (${res.status}).`);
        return;
      }
      setSource(body);
      setPending(null);
      setEditing(false);
      void refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mb-4">
      <h3 className="text-[11px] font-semibold text-accent uppercase tracking-wider mb-2">
        Source
      </h3>

      {editing ? (
        <div className="space-y-2">
          <input
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder="/path/to/repo or https://github.com/org/repo.git"
            autoFocus
            className="w-full px-2 py-1.5 bg-elevated border border-border-subtle bevel-sm text-text-primary text-xs font-mono focus:outline-none focus:border-accent"
          />
          <div className="flex gap-2">
            <button
              type="button"
              onClick={() => setPending({ path: draft.trim(), mode: "repoint" })}
              disabled={!draft.trim() || draft.trim() === source.path}
              className="px-3 py-1 text-xs bevel-sm bg-accent/15 text-accent border border-accent/40 hover:bg-accent/25 transition-colors disabled:opacity-35"
            >
              Save
            </button>
            <button
              type="button"
              onClick={() => { setEditing(false); setError(null); }}
              className="px-3 py-1 text-xs bevel-sm border border-border-subtle text-text-muted hover:text-text-primary transition-colors"
            >
              Cancel
            </button>
          </div>
        </div>
      ) : (
        <>
          <div className="flex items-start gap-2">
            <div className="flex-1 min-w-0 bevel-sm border border-border-subtle bg-root/40 px-2 py-1.5">
              <div className="text-[9px] uppercase tracking-wider text-text-muted mb-0.5">
                {source.kind === "git" ? "Git repository" : "Local path"}
              </div>
              <div className="font-mono text-[11px] text-text-secondary break-all">
                {source.path || "— not set —"}
              </div>
            </div>
            <button
              type="button"
              disabled={locked}
              onClick={() => { setDraft(source.path); setEditing(true); }}
              className="px-2 py-1 text-[10px] uppercase tracking-wider bevel-sm border border-border-subtle text-text-muted hover:text-accent hover:border-accent/40 transition-colors disabled:opacity-35 disabled:cursor-not-allowed"
            >
              Edit
            </button>
          </div>

          {/* Re-index the SAME source. Editing the path was the only way to
              trigger a rebuild, which meant the common case — the code on disk
              moved on and the graph didn't — had no button at all, since Save
              is disabled when the path is unchanged. The endpoint never
              required a different path; only the UI did. */}
          <button
            type="button"
            disabled={locked || !source.path}
            onClick={() => setPending({ path: source.path, mode: "reindex" })}
            title={
              locked
                ? "A rebuild is already running."
                : source.path
                  ? "Discard this project's index and build it again from the same source"
                  : "No source path is set for this project."
            }
            className="mt-2 w-full px-2 py-1.5 text-[10px] uppercase tracking-wider bevel-sm border border-border-subtle text-text-muted hover:text-accent hover:border-accent/40 transition-colors disabled:opacity-35 disabled:cursor-not-allowed"
          >
            Re-index
          </button>
        </>
      )}

      <SourceStatus source={source} />

      {pending && (
        <ConfirmRebuild
          title={pending.mode === "reindex" ? "Re-index this project?" : "Change project source?"}
          path={pending.path}
          kind={/^(https?:\/\/|git@|ssh:\/\/)/i.test(pending.path) ? "git" : "local"}
          busy={busy}
          error={error}
          onCancel={() => { setPending(null); setError(null); }}
          onConfirm={() => void submit(pending.path)}
        />
      )}
    </div>
  );
}


/** Same resolution order as App.tsx and ProjectOverview. */
function currentScope(): { project: string; stream: string } {
  const p = new URLSearchParams(window.location.search);
  return {
    project: p.get("project") || localStorage.getItem("prx-project") || "default",
    stream: p.get("stream") || localStorage.getItem("prx-stream") || "main",
  };
}

/**
 * Greys out the graph for as long as the project is being re-indexed.
 *
 * While a rebuild runs, the nodes on screen describe a codebase that is being
 * replaced underneath them: the chunks, symbols and edges they were derived
 * from are already gone. Leaving them live and clickable invites someone to
 * read, cite or navigate a graph that no longer matches anything on disk. The
 * veil says so and, because it covers the canvas, also swallows the clicks.
 *
 * It polls through useProjectSource, which stops polling by itself the moment
 * the status leaves "indexing" — so this costs nothing on an idle dashboard.
 */
export function GraphIndexingVeil({ token }: { token: string | null }) {
  const { project, stream } = currentScope();
  const { source } = useProjectSource(project, stream, token);

  if (source?.status !== "indexing") return null;

  return (
    <div
      className="absolute inset-0 z-40 flex items-center justify-center bg-root/75 backdrop-blur-[2px] cursor-progress"
      role="status"
      aria-live="polite"
      aria-busy="true"
    >
      <div className="text-center px-6 max-w-sm">
        <div className="prx-heading text-sm uppercase tracking-[0.12em] text-accent mb-2">
          Re-indexing
        </div>
        <p className="text-sm text-text-secondary">
          {source.message || "Rebuilding the index and graph…"}
        </p>
        <p className="text-xs text-text-muted mt-2">
          The graph shown underneath is from the previous index and is no longer
          accurate. It will reappear when the rebuild finishes.
        </p>
      </div>
    </div>
  );
}
