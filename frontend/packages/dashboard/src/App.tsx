import { useEffect, useState, useMemo, useCallback, lazy, Suspense } from "react";
import { validateGraph } from "@prx/core/schema";
import type { GraphIssue } from "@prx/core/schema";
import { useDashboardStore } from "./store";
import GraphView from "./components/GraphView";
import { GraphIndexingVeil } from "./components/SourcePanel";
import DomainGraphView from "./components/DomainGraphView";
import KnowledgeGraphView from "./components/KnowledgeGraphView";
import SearchBar from "./components/SearchBar";
import NodeInfo from "./components/NodeInfo";
import LayerLegend from "./components/LayerLegend";
import FilterPanel from "./components/FilterPanel";
import ExportMenu from "./components/ExportMenu";
import PersonaSelector from "./components/PersonaSelector";
import ProjectOverview from "./components/ProjectOverview";
import BusinessFlowModal from "./components/BusinessFlowModal";
import FileExplorer from "./components/FileExplorer";
import WarningBanner from "./components/WarningBanner";
import TokenGate from "./components/TokenGate";
import ProjectSwitcher from "./components/ProjectSwitcher";
import MobileLayout from "./components/MobileLayout";
import Wordmark from "./components/Wordmark";
import { useIsMobile } from "./hooks/useIsMobile";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import type { KeyboardShortcut } from "./hooks/useKeyboardShortcuts";
import { ThemeProvider } from "./themes/index.ts";
import { ThemePicker } from "./components/ThemePicker.tsx";
import type { ThemeConfig } from "./themes/index.ts";
import { I18nProvider, useI18n } from "./contexts/I18nContext.tsx";

// Lazy-load heavy / optional components so they ship in separate chunks.
const CodeViewer = lazy(() => import("./components/CodeViewer"));
const LearnPanel = lazy(() => import("./components/LearnPanel"));
const PathFinderModal = lazy(() => import("./components/PathFinderModal"));
const InsightAdvisor = lazy(() => import("./components/InsightAdvisor"));
const KeyboardShortcutsHelp = lazy(
  () => import("./components/KeyboardShortcutsHelp"),
);
const OnboardingOverlay = lazy(() => import("./components/OnboardingOverlay"));

const DEMO_MODE = import.meta.env.VITE_DEMO_MODE === "true";
/** Backend origin. Empty string means "same origin", which is what the Vite proxy gives us. */
const API_BASE = import.meta.env.VITE_API_BASE ?? "";
const SESSION_TOKEN_KEY = "prx-token";
const ONBOARDING_DISMISSED_KEY = "prx-onboarding-dismissed-v1";
type SidebarTab = "info" | "files" | "tour";

function shouldShowOnboarding(): boolean {
  if (typeof window === "undefined") return false;
  const params = new URLSearchParams(window.location.search);
  if (params.get("onboard") === "force") return true;
  return window.localStorage.getItem(ONBOARDING_DISMISSED_KEY) !== "1";
}

/**
 * The single seam every data fetch goes through.
 *
 * Upstream this resolved to a static file served by the Vite dev-server
 * middleware, gated on a one-time process token. Here it resolves to the
 * Praxevia Explorer backend, which serves the same five URLs as LOD fragments
 * — so the port is a re-point, not a rewrite.
 *
 * The project+stream selection rides on the query string because the backend is
 * multi-project, which the upstream single-repo dashboard had no concept of.
 */
function dataUrl(fileName: string, token: string | null): string {
  if (DEMO_MODE) {
    const envMap: Record<string, string | undefined> = {
      "knowledge-graph.json": import.meta.env.VITE_GRAPH_URL,
      "domain-graph.json": import.meta.env.VITE_DOMAIN_GRAPH_URL,
      "meta.json": import.meta.env.VITE_META_URL,
      "config.json": import.meta.env.VITE_CONFIG_URL,
    };
    const url = envMap[fileName];
    if (url) return url;
    const base = import.meta.env.BASE_URL || "/";
    return `${base.endsWith("/") ? base : `${base}/`}${fileName}`;
  }

  const { project, stream } = resolveProjectStream();
  const params = new URLSearchParams({ project, stream });
  // Auth moves to the Authorization header (see authHeaders); the token stays
  // out of the URL so it cannot leak via referrer or server logs.
  void token;
  return `${API_BASE}/api/compat/${fileName}?${params.toString()}`;
}

/** Bearer auth, replacing upstream's `?token=` query parameter. */
function authHeaders(token: string | null): HeadersInit {
  return token ? { Authorization: `Bearer ${token}` } : {};
}

/**
 * Which project+stream this dashboard is looking at.
 *
 * Read from the URL so a graph view is linkable, with a localStorage fallback so
 * a reload keeps its place. Defaults exist only to make the dev server usable
 * without arguments.
 */
const DEFAULT_PROJECT = "default";
const DEFAULT_STREAM = "main";

function resolveProjectStream(): { project: string; stream: string } {
  const params = new URLSearchParams(window.location.search);
  const fromUrl = params.get("project");
  const stored = localStorage.getItem("prx-project");
  // The last-resort name is a PLACEHOLDER, not a real project: it exists so the
  // first request has a well-formed URL. ProjectSwitcher replaces it with the
  // first graph the server actually has. It must never be the name of a real
  // project — a hardcoded one is a 404 on every deployment that lacks it, and
  // it leaks whichever codebase the developer happened to be indexing.
  const project = fromUrl || stored || DEFAULT_PROJECT;
  const stream =
    params.get("stream") || localStorage.getItem("prx-stream") || DEFAULT_STREAM;

  // Persist a real CHOICE, never the fallback. Writing the placeholder would
  // make it indistinguishable from something the user picked, so nothing
  // downstream could tell "they chose this" from "nobody has chosen yet", and
  // ProjectSwitcher could not land a first-time visitor on a graph that exists.
  if (fromUrl || stored) {
    localStorage.setItem("prx-project", project);
    localStorage.setItem("prx-stream", stream);
  }
  return { project, stream };
}

/**
 * Resolve the access token from the URL query string or sessionStorage.
 * If found in the URL, persist to sessionStorage and strip the param from the address bar.
 */
function resolveInitialToken(): string | null {
  if (DEMO_MODE) return "__demo__";
  const params = new URLSearchParams(window.location.search);
  const urlToken = params.get("token");
  if (urlToken) {
    sessionStorage.setItem(SESSION_TOKEN_KEY, urlToken);
    // Clean the URL
    params.delete("token");
    const cleanSearch = params.toString();
    const newUrl =
      window.location.pathname + (cleanSearch ? `?${cleanSearch}` : "") + window.location.hash;
    window.history.replaceState(null, "", newUrl);
    return urlToken;
  }
  return sessionStorage.getItem(SESSION_TOKEN_KEY);
}

function App() {
  const [accessToken, setAccessToken] = useState<string | null>(resolveInitialToken);

  const handleTokenValid = useCallback((token: string) => {
    sessionStorage.setItem(SESSION_TOKEN_KEY, token);
    setAccessToken(token);
  }, []);

  // In demo mode, skip token gate entirely
  if (DEMO_MODE) {
    return <Dashboard accessToken="__demo__" />;
  }

  // Show the token gate when no token is available
  if (accessToken === null) {
    return <TokenGate onTokenValid={handleTokenValid} />;
  }

  return <Dashboard accessToken={accessToken} />;
}

function Dashboard({ accessToken }: { accessToken: string }) {
  const setGraph = useDashboardStore((s) => s.setGraph);
  const setDomainGraph = useDashboardStore((s) => s.setDomainGraph);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [graphIssues, setGraphIssues] = useState<GraphIssue[]>([]);
  const [metaTheme, setMetaTheme] = useState<ThemeConfig | null>(null);
  const [outputLanguage, setOutputLanguage] = useState<string | undefined>();

  useEffect(() => {
    const headers = authHeaders(accessToken);
    fetch(dataUrl("meta.json", accessToken), { headers })
      .then((r) => (r.ok ? r.json() : null))
      .then((meta) => {
        if (meta?.theme) setMetaTheme(meta.theme);
      })
      .catch(() => {});
    fetch(dataUrl("config.json", accessToken), { headers })
      .then((r) => (r.ok ? r.json() : null))
      .then((config) => {
        if (config?.outputLanguage) setOutputLanguage(config.outputLanguage);
      })
      .catch(() => {});
  }, []);

  // Land on a graph that EXISTS, before anything tries to load one.
  //
  // This must live here, not in ProjectSwitcher: the switcher only renders once
  // a graph is loaded (`projectName` comes from `graph`), so on a deployment
  // whose stored/placeholder project does not exist, the switcher never mounted,
  // its redirect never ran, and the app hung on an empty canvas forever. The
  // placeholder default made that the FIRST-LOAD path for every new install.
  useEffect(() => {
    if (!accessToken || DEMO_MODE) return;
    fetch(`${API_BASE}/api/graph/projects`, { headers: authHeaders(accessToken) })
      .then((r) => (r.ok ? r.json() : null))
      .then((body) => {
        const graphs: string[] = body?.graphs ?? [];
        if (graphs.length === 0) return;
        const params = new URLSearchParams(window.location.search);
        const chosen = params.get("project") || localStorage.getItem("prx-project");
        const known = new Set(graphs.map((g) => g.slice(0, g.lastIndexOf("_"))));
        if (chosen && known.has(chosen)) return;
        const first = graphs[0];
        const cut = first.lastIndexOf("_");
        params.set("project", first.slice(0, cut));
        params.set("stream", first.slice(cut + 1));
        window.location.search = params.toString();
      })
      .catch(() => {});
  }, [accessToken]);

  useEffect(() => {
    fetch(dataUrl("knowledge-graph.json", accessToken), {
      headers: authHeaders(accessToken),
    })
      .then(async (res) => {
        // Upstream omits this check on THIS endpoint only, while checking it on
        // every other one. The consequence is that a 401/403 gets
        // parsed as a graph and surfaces to the user as "Invalid knowledge
        // graph" — a wrong diagnosis for an auth problem.
        if (!res.ok) {
          const detail = await res
            .json()
            .then((b) => b?.detail)
            .catch(() => null);
          if (res.status === 401) throw new Error("Not signed in, or the session expired.");
          if (res.status === 403) throw new Error("You do not have access to this project.");
          if (res.status === 404) {
            throw new Error(
              detail ?? "No graph has been built for this project yet.",
            );
          }
          throw new Error(detail ?? `Backend returned ${res.status}`);
        }
        return res.json();
      })
      .then((data: unknown) => {
        const result = validateGraph(data);
        if (result.success && result.data) {
          setGraph(result.data);
          setGraphIssues(result.issues);
          if ((data as Record<string, unknown>).kind === "knowledge") {
            useDashboardStore.getState().setViewMode("knowledge");
            useDashboardStore.getState().setIsKnowledgeGraph(true);
          }
          for (const issue of result.issues) {
            if (issue.level === "auto-corrected") {
              console.warn(`[graph] auto-corrected: ${issue.message}`);
            } else if (issue.level === "dropped") {
              console.error(`[graph] dropped: ${issue.message}`);
            }
          }
        } else if (result.fatal) {
          console.error("Knowledge graph validation failed:", result.fatal);
          setLoadError(`Invalid knowledge graph: ${result.fatal}`);
        } else {
          console.error("Knowledge graph validation failed: unknown error");
          setLoadError("Invalid knowledge graph: unknown validation error");
        }
      })
      .catch((err) => {
        console.error("Failed to load knowledge graph:", err);
        // The message is already diagnostic (auth vs. no-graph vs. transport),
        // so surface it directly rather than wrapping it in a second layer.
        setLoadError(err instanceof Error ? err.message : String(err));
      });
  }, [setGraph]);

  // A 404 here is an ordinary state, not an error: it means nobody has
  // generated business flows for this project yet, and the domain toggle simply
  // stays hidden. Anything else is swallowed for the same reason — this fetch
  // must never be able to break the structural view it runs alongside.
  const loadDomainGraph = useCallback(async (): Promise<boolean> => {
    try {
      const res = await fetch(dataUrl("domain-graph.json", accessToken), {
        headers: authHeaders(accessToken),
      });
      if (!res.ok) return false;
      const data: unknown = await res.json();
      const result = validateGraph(data);
      if (result.success && result.data) {
        setDomainGraph(result.data);
        return true;
      }
      if (result.fatal) {
        console.warn(`[domain-graph] validation failed: ${result.fatal}`);
      }
      return false;
    } catch {
      return false;
    }
  }, [accessToken, setDomainGraph]);

  useEffect(() => {
    void loadDomainGraph();
  }, [loadDomainGraph]);

  // Poll while a generation job is running.
  //
  // Polling rather than a socket for the same reason SourcePanel polls its
  // rebuild: the job is minutes long and rare, a 3-second poll costs nothing and
  // cannot get stuck half-open, and it stops as soon as the status settles.
  const businessFlowStatus = useDashboardStore((s) => s.businessFlowStatus);
  const setBusinessFlowStatus = useDashboardStore((s) => s.setBusinessFlowStatus);

  useEffect(() => {
    if (businessFlowStatus !== "generating" || DEMO_MODE) return;
    const { project, stream } = resolveProjectStream();
    let cancelled = false;

    const tick = async () => {
      try {
        const params = new URLSearchParams({ project, stream });
        const res = await fetch(
          `${API_BASE}/api/graph/business-flow?${params.toString()}`,
          { headers: authHeaders(accessToken) },
        );
        if (!res.ok || cancelled) return;
        const row = await res.json();
        if (cancelled) return;
        if (row.status === "ready") {
          await loadDomainGraph();
          setBusinessFlowStatus("ready");
        } else if (row.status === "failed") {
          setBusinessFlowStatus("failed", row.message || null);
        }
      } catch {
        // A dropped poll is not a failed job; the next tick retries.
      }
    };

    const id = setInterval(tick, 3000);
    void tick();
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [businessFlowStatus, accessToken, loadDomainGraph, setBusinessFlowStatus]);

  return (
    <I18nProvider language={outputLanguage ?? "en"}>
      <ThemeProvider metaTheme={metaTheme}>
        <DashboardContent
          accessToken={accessToken}
          loadError={loadError}
          graphIssues={graphIssues}
        />
        {/* Mounted here rather than inside ProjectOverview: the panel that opens
            it lives in a sidebar tab, and switching tabs would unmount the
            dialog mid-interaction. */}
        {!DEMO_MODE && (
          <BusinessFlowModal
            project={resolveProjectStream().project}
            stream={resolveProjectStream().stream}
            token={accessToken}
          />
        )}
      </ThemeProvider>
    </I18nProvider>
  );
}

function DashboardContent({
  accessToken,
  loadError,
  graphIssues,
}: {
  accessToken: string;
  loadError: string | null;
  graphIssues: GraphIssue[];
}) {
  const graph = useDashboardStore((s) => s.graph);
  const selectedNodeId = useDashboardStore((s) => s.selectedNodeId);
  const tourActive = useDashboardStore((s) => s.tourActive);
  const persona = useDashboardStore((s) => s.persona);
  const codeViewerOpen = useDashboardStore((s) => s.codeViewerOpen);
  const codeViewerExpanded = useDashboardStore((s) => s.codeViewerExpanded);
  const expandCodeViewer = useDashboardStore((s) => s.expandCodeViewer);
  const collapseCodeViewer = useDashboardStore((s) => s.collapseCodeViewer);
  const pathFinderOpen = useDashboardStore((s) => s.pathFinderOpen);
  const togglePathFinder = useDashboardStore((s) => s.togglePathFinder);
  const nodeTypeFilters = useDashboardStore((s) => s.nodeTypeFilters);
  const toggleNodeTypeFilter = useDashboardStore((s) => s.toggleNodeTypeFilter);
  const detailLevel = useDashboardStore((s) => s.detailLevel);
  const setDetailLevel = useDashboardStore((s) => s.setDetailLevel);
  const showFunctionsInClassView = useDashboardStore((s) => s.showFunctionsInClassView);
  const toggleShowFunctionsInClassView = useDashboardStore((s) => s.toggleShowFunctionsInClassView);
  const [showKeyboardHelp, setShowKeyboardHelp] = useState(false);
  const [sidebarTab, setSidebarTab] = useState<SidebarTab>("info");
  const [showOnboarding, setShowOnboarding] = useState(shouldShowOnboarding);
  const dismissOnboarding = useCallback((remember: boolean) => {
    if (remember && typeof window !== "undefined") {
      window.localStorage.setItem(ONBOARDING_DISMISSED_KEY, "1");
    }
    setShowOnboarding(false);
  }, []);
  const viewMode = useDashboardStore((s) => s.viewMode);
  const setViewMode = useDashboardStore((s) => s.setViewMode);
  const isKnowledgeGraph = useDashboardStore((s) => s.isKnowledgeGraph);
  const domainGraph = useDashboardStore((s) => s.domainGraph);
  const layoutIssues = useDashboardStore((s) => s.layoutIssues);
  const isMobile = useIsMobile();
  const { t } = useI18n();
  const allIssues = useMemo(
    () => [...graphIssues, ...layoutIssues],
    [graphIssues, layoutIssues],
  );

  // The header shows the "Praxevia Insight" brand lockup, then the analyzed
  // project's name as a secondary label. Suppress that label when it just
  // repeats the product name (e.g. a project literally named "Insight", or the
  // bundled sample) so the header never reads "Praxevia Insight · Insight".
  const projectName = graph?.project.name?.trim();
  const showProjectName =
    !!projectName &&
    projectName.toLowerCase() !== t.common.appName.trim().toLowerCase();

  // Declared here rather than beside the sidebar markup because the effects
  // below list it as a dependency, and a dependency array is evaluated during
  // render — a `const` further down the component would still be in its
  // temporal dead zone at that point and throw.
  const isLearnMode = tourActive || persona === "junior";

  // Selecting a node means "show me this node", so jump to INFO — including
  // from a tour step's component pills, which exist precisely to open the node
  // they name. Safe during a tour: advancing a step sets the highlight and the
  // layer but never `selectedNodeId` (see startTour/setTourStep in store.ts),
  // so walking the tour cannot pull the reader off the panel. Tour state
  // survives the trip, and the TOUR tab resumes on the same step.
  useEffect(() => {
    if (selectedNodeId) setSidebarTab("info");
  }, [selectedNodeId]);

  // Leaving Learn takes the TOUR tab with it. Without this the tab strip would
  // render two buttons while `sidebarTab` still said "tour", so the panel would
  // keep showing a tour no visible tab was selecting.
  useEffect(() => {
    if (!isLearnMode) {
      setSidebarTab((current) => (current === "tour" ? "info" : current));
    }
  }, [isLearnMode]);

  // Define keyboard shortcuts
  const shortcuts = useMemo<KeyboardShortcut[]>(
    () => [
      // Help
      {
        key: "?",
        shiftKey: true,
        description: t.keyboardShortcuts.showHelp,
        action: () => setShowKeyboardHelp((prev) => !prev),
        category: "General",
      },
      // Navigation
      {
        key: "Escape",
        description: t.keyboardShortcuts.escapeDesc,
        action: () => {
          // Read from store at invocation time to avoid stale closures
          const state = useDashboardStore.getState();
          if (state.pathFinderOpen) {
            state.togglePathFinder();
          } else if (state.filterPanelOpen) {
            state.toggleFilterPanel();
          } else if (state.exportMenuOpen) {
            state.toggleExportMenu();
          } else if (state.codeViewerExpanded) {
            state.collapseCodeViewer();
          } else if (state.codeViewerOpen) {
            state.closeCodeViewer();
          } else if (state.selectedNodeId) {
            state.selectNode(null);
          } else if (state.navigationLevel === "layer-detail") {
            state.navigateToOverview();
          } else if (state.tourActive) {
            state.stopTour();
          } else {
            setShowKeyboardHelp(false);
          }
        },
        category: "Navigation",
      },
      {
        key: "/",
        description: t.keyboardShortcuts.focusSearch,
        action: () => {
          const searchInput = document.querySelector<HTMLInputElement>(
            '[data-testid="search-input"]'
          );
          searchInput?.focus();
        },
        category: "Navigation",
      },
      // Tour controls
      {
        key: "ArrowRight",
        description: t.keyboardShortcuts.nextStep,
        action: () => {
          const state = useDashboardStore.getState();
          if (state.tourActive) {
            state.nextTourStep();
          }
        },
        category: "Tour",
      },
      {
        key: "ArrowLeft",
        description: t.keyboardShortcuts.prevStep,
        action: () => {
          const state = useDashboardStore.getState();
          if (state.tourActive) {
            state.prevTourStep();
          }
        },
        category: "Tour",
      },
      // View toggles
      {
        key: "f",
        description: t.keyboardShortcuts.toggleFilter,
        action: () => {
          const state = useDashboardStore.getState();
          state.toggleFilterPanel();
        },
        category: "View",
      },
      {
        key: "e",
        description: t.keyboardShortcuts.toggleExport,
        action: () => {
          const state = useDashboardStore.getState();
          state.toggleExportMenu();
        },
        category: "View",
      },
      {
        key: "p",
        description: t.keyboardShortcuts.openPathFinder,
        action: () => {
          const state = useDashboardStore.getState();
          state.togglePathFinder();
        },
        category: "View",
      },
    ],
    [t]
  );

  // Register keyboard shortcuts
  useKeyboardShortcuts(shortcuts);

  // Determine sidebar content.
  //
  // NodeInfo takes priority when a node is selected; with nothing selected the
  // panel describes the WHOLE codebase. Learn mode adds LearnPanel below either.
  //
  // ProjectOverview used to be suppressed whenever Learn was active
  // (`&& !isLearnMode`), so selecting a repository in Learn — the DEFAULT
  // persona — left the INFO tab showing "No tour available" and nothing else:
  // no name, no counts, no source path, on a panel whose whole job is to say
  // what you are looking at. A persona chooses what to ADD, never whether the
  // subject is identified at all.
  // INFO answers "what am I looking at". The tour is a MODE, and it now has its
  // own tab rather than being appended underneath — stacked below the overview
  // it was reachable only by scrolling past every statistic on the panel.
  const infoSidebarContent = selectedNodeId ? (
    <NodeInfo />
  ) : (
    <ProjectOverview token={accessToken} />
  );

  // TOUR appears only in Learn — it is what the Learn persona is for. `tourActive`
  // is in the condition as well so that switching back to Overview mid-tour does
  // not delete the panel driving it and strand the user on step 4.
  const sidebarTabs: SidebarTab[] = isLearnMode
    ? ["info", "files", "tour"]
    : ["info", "files"];
  const tabLabel: Record<SidebarTab, string> = {
    info: t.sidebar.info,
    files: t.sidebar.files,
    tour: t.sidebar.tour,
  };

  const sidebarContent = (
    <div className="h-full flex flex-col min-h-0">
      <div className="flex items-center gap-1 p-2 border-b border-border-subtle bg-surface shrink-0">
        {sidebarTabs.map((tab) => (
          <button
            key={tab}
            type="button"
            onClick={() => setSidebarTab(tab)}
            className={`flex-1 px-3 py-1.5 bevel-sm text-xs font-semibold uppercase tracking-wider transition-colors ${
              sidebarTab === tab
                ? "bg-accent/15 text-accent"
                : "text-text-muted hover:text-text-primary hover:bg-elevated"
            }`}
          >
            {tabLabel[tab]}
          </button>
        ))}
      </div>
      <div className="flex-1 min-h-0 overflow-auto">
        {sidebarTab === "files" ? (
          <FileExplorer />
        ) : sidebarTab === "tour" ? (
          <Suspense fallback={null}>
            <LearnPanel />
          </Suspense>
        ) : (
          infoSidebarContent
        )}
      </div>
    </div>
  );

  if (isMobile) {
    return (
      <MobileLayout
        accessToken={accessToken}
        showKeyboardHelp={showKeyboardHelp}
        setShowKeyboardHelp={setShowKeyboardHelp}
        loadError={loadError}
        allIssues={allIssues}
        shortcuts={shortcuts}
      />
    );
  }

  return (
    <div className="h-screen w-screen flex flex-col bg-root text-text-primary noise-overlay">
      {/* Header */}
      <header className="flex items-center px-3 sm:px-5 py-3 bg-surface border-b border-border-subtle shrink-0 gap-2 sm:gap-4">
        {/* Left — fixed */}
        <div className="flex items-center gap-3 sm:gap-5 shrink-0 min-w-0">
          <div className="flex items-center gap-2 sm:gap-3 min-w-0">
            <h1 className="shrink-0 m-0 leading-none">
              <Wordmark
                logoClassName="h-3.5 sm:h-[17px]"
                nameClassName="text-base sm:text-lg"
              />
            </h1>
            {showProjectName && (
              <>
                {/* Secondary project label is shown only at lg+ where there's
                    horizontal room. Below that it is hidden so a long project
                    name can never push the fixed header actions off-screen at
                    narrow desktop widths (~768–1024px). */}
                <span
                  className="w-px h-4 bg-border-subtle hidden lg:block shrink-0"
                  aria-hidden="true"
                />
                <ProjectSwitcher current={projectName!} token={accessToken} />
              </>
            )}
          </div>
          <div className="w-px h-5 bg-border-subtle hidden sm:block" />
          <PersonaSelector />
          {graph && !isKnowledgeGraph && domainGraph && (
            <>
              <div className="w-px h-5 bg-border-subtle" />
              <div className="flex items-center bg-elevated bevel-sm p-0.5">
                <button
                  type="button"
                  onClick={() => setViewMode("domain")}
                  title={t.drawer.domain}
                  className={`px-3 py-1 text-xs font-medium bevel-sm transition-colors ${
                    viewMode === "domain"
                      ? "bg-accent/20 text-accent"
                      : "text-text-muted hover:text-text-secondary"
                  }`}
                >
                  {t.drawer.domain}
                </button>
                <button
                  type="button"
                  onClick={() => setViewMode("structural")}
                  title={t.drawer.structural}
                  className={`px-3 py-1 text-xs font-medium bevel-sm transition-colors ${
                    viewMode === "structural"
                      ? "bg-accent/20 text-accent"
                      : "text-text-muted hover:text-text-secondary"
                  }`}
                >
                  {t.drawer.structural}
                </button>
              </div>
            </>
          )}
        </div>

        {/* Middle — scrollable legends */}
        <div className="flex-1 min-w-0 overflow-x-auto scrollbar-hide">
          <div className="flex items-center gap-4 w-max">
            {/* Detail level: file view (architecture) / class view (code structure) */}
            {!isKnowledgeGraph && viewMode !== "domain" && (
              <>
                <div className="w-px h-5 bg-border-subtle" />
                <div className="flex items-center bg-elevated bevel-sm p-0.5">
                  <button
                    type="button"
                    onClick={() => setDetailLevel("file")}
                    title={t.detailLevel.filesTitle}
                    className={`px-3 py-1 text-xs font-medium bevel-sm transition-colors ${
                      detailLevel === "file"
                        ? "bg-accent/20 text-accent"
                        : "text-text-muted hover:text-text-secondary"
                    }`}
                  >
                    {t.detailLevel.files}
                  </button>
                  <button
                    type="button"
                    onClick={() => setDetailLevel("class")}
                    title={t.detailLevel.classesTitle}
                    className={`px-3 py-1 text-xs font-medium bevel-sm transition-colors ${
                      detailLevel === "class"
                        ? "bg-accent/20 text-accent"
                        : "text-text-muted hover:text-text-secondary"
                    }`}
                  >
                    {t.detailLevel.classes}
                  </button>
                </div>
                {detailLevel === "class" && (
                  <button
                    type="button"
                    onClick={toggleShowFunctionsInClassView}
                    title={t.detailLevel.fnTitle}
                    className={`text-[10px] font-semibold uppercase tracking-wider px-2 py-1 bevel-sm border transition-colors ${
                      showFunctionsInClassView
                        ? "border-amber-500/50 bg-amber-500/10 text-amber-400"
                        : "border-border-medium bg-elevated text-text-muted hover:text-text-secondary"
                    }`}
                  >
                    {t.detailLevel.fn}
                  </button>
                )}
              </>
            )}
            <div className="flex items-center gap-1">
              {(isKnowledgeGraph ? [
                { key: "knowledge" as const, label: t.nodeTypeLabels.all, color: "var(--color-node-article)" },
              ] : [
                { key: "code" as const, label: t.nodeTypeLabels.code, color: "var(--color-node-file)" },
                { key: "config" as const, label: t.nodeTypeLabels.config, color: "var(--color-node-config)" },
                { key: "docs" as const, label: t.nodeTypeLabels.docs, color: "var(--color-node-document)" },
                { key: "infra" as const, label: t.nodeTypeLabels.infra, color: "var(--color-node-service)" },
                { key: "data" as const, label: t.nodeTypeLabels.data, color: "var(--color-node-table)" },
                { key: "domain" as const, label: t.nodeTypeLabels.domain, color: "var(--color-node-concept)" },
                { key: "knowledge" as const, label: t.nodeTypeLabels.knowledge, color: "var(--color-node-article)" },
              ]).map((cat) => (
                <button
                  key={cat.key}
                  onClick={() => toggleNodeTypeFilter(cat.key)}
                  className={`text-[10px] font-semibold uppercase tracking-wider px-2 py-1 bevel-sm border transition-colors flex items-center gap-1.5 whitespace-nowrap ${
                    nodeTypeFilters[cat.key] !== false
                      ? "border-border-medium bg-elevated text-text-secondary hover:text-text-primary"
                      : "border-transparent bg-transparent text-text-muted/40 line-through hover:text-text-muted"
                  }`}
                  title={`${nodeTypeFilters[cat.key] !== false ? "Hide" : "Show"} ${cat.label} nodes`}
                >
                  <span
                    className="w-2 h-2 rounded-full shrink-0"
                    style={{
                      backgroundColor: cat.color,
                      opacity: nodeTypeFilters[cat.key] !== false ? 1 : 0.3,
                    }}
                  />
                  {cat.label}
                </button>
              ))}
            </div>
          </div>
        </div>

        {/* Right — fixed actions */}
        <div className="flex items-center gap-2 sm:gap-4 shrink-0">
          <FilterPanel />
          <ExportMenu />
          <button
            onClick={togglePathFinder}
            className="flex items-center gap-1.5 px-2 sm:px-3 py-1.5 bevel-sm text-sm bg-elevated text-text-secondary hover:text-text-primary transition-colors"
            title={t.pathFinder.title}
          >
            <svg
              className="w-4 h-4"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M13 7h8m0 0v8m0-8l-8 8-4-4-6 6"
              />
            </svg>
            <span className="hidden md:inline">{t.common.path}</span>
          </button>
          <ThemePicker />
          <button
            onClick={() => setShowKeyboardHelp(true)}
            className="text-text-muted hover:text-accent transition-colors"
            title={t.keyboardShortcuts.showHelp}
          >
            <svg
              className="w-5 h-5"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M8.228 9c.549-1.165 2.03-2 3.772-2 2.21 0 4 1.343 4 3 0 1.4-1.278 2.575-3.006 2.907-.542.104-.994.54-.994 1.093m0 3h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
              />
            </svg>
          </button>
        </div>
      </header>

      {/* Layer legend — its own full-width row so the layer list has room and
          never gets clipped by the fixed header actions (Filter / Export / …).
          Renders only when the graph defines layers. */}
      {(graph?.layers?.length ?? 0) > 0 && (
        <div className="shrink-0 bg-surface border-b border-border-subtle px-3 sm:px-5 py-1.5 overflow-x-auto scrollbar-hide">
          <div className="w-max">
            <LayerLegend />
          </div>
        </div>
      )}

      {/* Search */}
      <SearchBar />

      {/* Validation warning banner */}
      {allIssues.length > 0 && !loadError && (
        <WarningBanner issues={allIssues} />
      )}

      {/* Error banner */}
      {loadError && (
        <div className="px-5 py-3 bg-red-900/30 border-b border-red-700 text-red-200 text-sm">
          {loadError}
        </div>
      )}

      {/* Main content: Graph + Sidebar */}
      <div className="flex-1 flex min-h-0 relative">
        {/* Graph area */}
        {/* Tagged so the expanded chat can measure the graph area instead of
            guessing at it. Hardcoded offsets were wrong at every viewport
            except the one they were written on. */}
        <div data-graph-canvas className="flex-1 min-w-0 min-h-0 relative">
          {/* Covers the canvas while this project is re-indexing. */}
          <GraphIndexingVeil token={accessToken} />
          {viewMode === "knowledge" ? (
            <KnowledgeGraphView />
          ) : viewMode === "domain" && domainGraph ? (
            <DomainGraphView />
          ) : (
            <GraphView />
          )}
          <div className="absolute top-3 right-3 text-sm text-text-muted/60 pointer-events-none select-none">
            {t.common.pressKeyboard}
          </div>
        </div>

        {/* Right sidebar — telescopes at narrower widths */}
        <aside className="w-[260px] md:w-[300px] lg:w-[360px] shrink-0 bg-surface border-l border-border-subtle overflow-auto">
          {sidebarContent}
        </aside>

        {/* Code viewer slide-up overlay (collapsed state) */}
        {codeViewerOpen && !codeViewerExpanded && (
          <div className="absolute bottom-0 left-0 right-0 h-[40vh] bg-surface border-t border-border-subtle animate-slide-up z-20 overflow-hidden">
            <Suspense fallback={null}>
              <CodeViewer accessToken={accessToken} onExpand={expandCodeViewer} />
            </Suspense>
          </div>
        )}
      </div>

      {/* Expanded code viewer modal */}
      {codeViewerOpen && codeViewerExpanded && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/65 backdrop-blur-sm p-4 sm:p-6"
          onMouseDown={collapseCodeViewer}
        >
          <div
            className="w-[calc(100vw-32px)] max-w-[1120px] h-[calc(100vh-32px)] sm:h-[calc(100vh-48px)] max-h-[820px] bevel-sm border border-border-medium bg-surface bevel-elevate overflow-hidden"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <Suspense fallback={null}>
              <CodeViewer
                accessToken={accessToken}
                presentation="modal"
                onClose={collapseCodeViewer}
              />
            </Suspense>
          </div>
        </div>
      )}

      {/* Keyboard shortcuts help modal */}
      {showKeyboardHelp && (
        <Suspense fallback={null}>
          <KeyboardShortcutsHelp
            shortcuts={shortcuts}
            onClose={() => setShowKeyboardHelp(false)}
          />
        </Suspense>
      )}

      {/* Path Finder Modal — only mounted when open so its chunk is lazy-loaded on demand. */}
      {pathFinderOpen && (
        <Suspense fallback={null}>
          <PathFinderModal isOpen={pathFinderOpen} onClose={togglePathFinder} />
        </Suspense>
      )}

      {/* First-visit onboarding overlay — only mounted when needed so its chunk is lazy-loaded on demand. */}
      {showOnboarding && (
        <Suspense fallback={null}>
          <OnboardingOverlay onDismiss={dismissOnboarding} />
        </Suspense>
      )}

      {/* Insight Advisor — bottom-left codebase assistant. Lazy so its markdown
          chunk stays out of the main bundle; self-hides in the demo build. */}
      <Suspense fallback={null}>
        <InsightAdvisor accessToken={accessToken} />
      </Suspense>
    </div>
  );
}

export default App;
