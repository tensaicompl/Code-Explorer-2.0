import { useEffect, useRef, useState } from "react";

import { useDashboardStore } from "../store";
import { useI18n } from "../contexts/I18nContext";

/**
 * Asks how the business-flow graph should be generated, then starts the job.
 *
 * Closing this dialog does NOT cancel a running generation. The job lives on the
 * server and outlives any one browser tab, so treating the close button as a
 * cancel would lie about what it does. The header button reports progress once
 * the dialog is gone.
 */
export default function BusinessFlowModal({
  project,
  stream,
  token,
}: {
  project: string;
  stream: string;
  token: string | null;
}) {
  const open = useDashboardStore((s) => s.businessFlowModalOpen);
  const setOpen = useDashboardStore((s) => s.setBusinessFlowModalOpen);
  const setStatus = useDashboardStore((s) => s.setBusinessFlowStatus);
  const { t } = useI18n();

  const [mode, setMode] = useState<"deterministic" | "llm">("deterministic");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const modalRef = useRef<HTMLDivElement>(null);

  // Outside-click and Escape as two separate effects, matching PathFinderModal.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (modalRef.current && !modalRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open, setOpen]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, setOpen]);

  if (!open) return null;

  const submit = async () => {
    setSubmitting(true);
    setError(null);
    try {
      const res = await fetch(`${import.meta.env.VITE_API_BASE ?? ""}/api/graph/business-flow`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
        },
        body: JSON.stringify({ project, stream, mode }),
      });
      if (!res.ok) {
        const body = await res.json().catch(() => null);
        throw new Error(body?.detail || `Request failed (${res.status})`);
      }
      // The polling effect in App.tsx keys off this transition; it refetches the
      // graph and flips the status when the job settles.
      setStatus("generating");
      setOpen(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSubmitting(false);
    }
  };

  const options = [
    {
      id: "deterministic" as const,
      label: t.businessFlow.deterministic,
      description: t.businessFlow.deterministicDesc,
      disabled: false,
    },
    {
      id: "llm" as const,
      label: t.businessFlow.llm,
      description: t.businessFlow.llmDesc,
      disabled: false,
    },
  ];

  return (
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-root/80 backdrop-blur-sm px-4"
      role="dialog"
      aria-modal="true"
      aria-label={t.businessFlow.title}
    >
      <div
        ref={modalRef}
        className="glass-heavy bevel bevel-elevate w-full max-w-xl max-h-[85vh] overflow-auto animate-fade-slide-in"
      >
        <div className="px-6 pt-5 pb-3 border-b border-border-subtle">
          <h2 className="font-heading text-lg text-text-primary">{t.businessFlow.title}</h2>
          <p className="text-xs text-text-secondary leading-relaxed mt-1">
            {t.businessFlow.intro}
          </p>
        </div>

        <div className="p-5 space-y-3">
          {options.map((opt) => (
            <button
              key={opt.id}
              type="button"
              disabled={opt.disabled}
              onClick={() => setMode(opt.id)}
              className={`w-full text-left p-4 bevel-sm border transition-colors ${
                opt.disabled
                  ? "border-border-subtle bg-elevated/40 opacity-50 cursor-not-allowed"
                  : mode === opt.id
                  ? "border-accent bg-accent/10"
                  : "border-border-subtle bg-elevated hover:border-accent/40"
              }`}
            >
              <div className="flex items-center justify-between mb-1.5">
                <span className="text-sm font-medium text-text-primary">{opt.label}</span>
                {opt.disabled && (
                  <span className="text-[10px] uppercase tracking-wider text-text-muted">
                    {t.businessFlow.llmUnavailable}
                  </span>
                )}
              </div>
              <p className="text-xs text-text-secondary leading-relaxed">{opt.description}</p>
            </button>
          ))}

          {error && (
            <p className="text-xs text-red-400 leading-relaxed">{error}</p>
          )}
        </div>

        <div className="px-5 pb-5 flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={() => setOpen(false)}
            className="px-3 py-1.5 text-xs bevel-sm bg-elevated border border-border-subtle text-text-secondary hover:text-text-primary transition-colors"
          >
            {t.businessFlow.cancel}
          </button>
          <button
            type="button"
            onClick={submit}
            disabled={submitting}
            className="px-3 py-1.5 text-xs font-medium bevel-sm bg-cta text-white hover:opacity-90 disabled:opacity-50 transition-opacity"
          >
            {submitting ? t.businessFlow.generating : t.businessFlow.generate}
          </button>
        </div>
      </div>
    </div>
  );
}
