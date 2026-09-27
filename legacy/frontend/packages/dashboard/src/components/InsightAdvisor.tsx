import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import ReactMarkdown from "react-markdown";
import {
  buildHtml, buildMarkdown, downloadText, exportFilename,
} from "../utils/chatExport";
import remarkGfm from "remark-gfm";
import type { GraphNode } from "@prx/core/types";
import { useDashboardStore } from "../store";

// ~500 KB, and only ever needed once a diagram actually appears in a reply.
const MermaidDiagram = lazy(() => import("./MermaidDiagram"));

/**
 * Insight Advisor — a bottom-left chat assistant that answers questions about the
 * currently loaded codebase.
 *
 * How it stays grounded:
 *   1. The question goes straight to `/api/chat`, which owns the API key — the
 *      key never reaches the browser.
 *   2. The AGENT does the retrieval, with its own tools against the pgvector
 *      index. The browser sends no context blob. The earlier viewer had no backend, so
 *      it had to assemble one client-side; keeping that here would both waste
 *      tokens and cap the agent at whatever the browser happened to have loaded.
 *   3. The model cites what it found as `[[path]]` or `[[path:symbol]]`, and
 *      THIS FILE resolves those to graph nodes (see `buildRefIndex`) and renders
 *      them as chips that jump to the node and open the code viewer.
 *
 * Why the frontend resolves rather than the model emitting a node id: the
 * backend's tools return `filename` and `name`, never a node id. An id is
 * `{type}:{path}:{name}` where the type comes from a kind→type map the model
 * cannot see (`message`→class, `rpc`→endpoint,. Asking it to construct one
 * yields ids that resolve to nothing, and an unresolvable citation renders
 * silently as nothing — a feature that looks wired and never fires. The graph
 * lives here, so the mapping belongs here.
 *
 * The advisor is hidden in the static hosted demo (accessToken "__demo__"), which
 * has no server proxy and therefore no way to reach a provider.
 */

const MAX_HISTORY_TURNS = 8;
const REF_RE = /\[\[([^\]]+)\]\]/g;

interface ChatMessage {
  role: "user" | "assistant";
  content: string;
  /** Node ids to surface as deep-link chips (assistant messages only). */
  refs?: string[];
  isError?: boolean;
}

export interface RefIndex {
  /** Every node, by id — citations may already be exact. */
  byId: Map<string, GraphNode>;
  /** `path` (lowercased) → node id. Prefers the `file:` node for that path. */
  byPath: Map<string, string>;
  /** `path:name` (lowercased) → node id. First declaration wins. */
  byPathName: Map<string, string>;
  /** `name` (lowercased) → node id, only where the name is UNIQUE graph-wide. */
  byUniqueName: Map<string, string>;
}

/**
 * Build the lookup a citation is resolved against.
 *
 * `byUniqueName` deliberately drops any name held by more than one node. A
 * citation chip that jumps to one of four same-named functions is a guess
 * presented as a fact, which is the same rule the graph's own edges follow
 *. A path-qualified citation stays exact and is unaffected.
 */
export function buildRefIndex(nodesById: Map<string, GraphNode>): RefIndex {
  const byPath = new Map<string, string>();
  const byPathName = new Map<string, string>();
  const nameCounts = new Map<string, number>();
  const nameFirst = new Map<string, string>();

  for (const node of nodesById.values()) {
    const name = node.name?.toLowerCase();
    if (name) {
      nameCounts.set(name, (nameCounts.get(name) ?? 0) + 1);
      if (!nameFirst.has(name)) nameFirst.set(name, node.id);
    }
    const path = node.filePath?.toLowerCase();
    if (!path) continue;
    // A `file:` node is the right target for a bare path; anything else is a
    // symbol that merely lives there, so it must not hold the path slot once a
    // real file node exists — hence the overwrite rather than a `setdefault`.
    if (node.type === "file" || !byPath.has(path)) byPath.set(path, node.id);
    if (name) {
      const key = `${path}:${name}`;
      if (!byPathName.has(key)) byPathName.set(key, node.id);
    }
  }

  const byUniqueName = new Map<string, string>();
  for (const [name, count] of nameCounts) {
    if (count === 1) byUniqueName.set(name, nameFirst.get(name)!);
  }
  return { byId: nodesById, byPath, byPathName, byUniqueName };
}

/**
 * Resolve one citation to a node id, or null.
 *
 * Accepted forms, most specific first — the model is told to use the middle two
 * because they are what its tool results actually contain:
 *   `class:src/a.py:Widget`  an exact node id
 *   `src/a.py:Widget`        a path and a symbol in it
 *   `src/a.py`               a file
 *   `Widget`                 a bare name, ONLY if unique in the whole graph
 *
 * A trailing line range (`src/a.py:12-40`) is tolerated and stripped: it is the
 * shape `find_symbol_references` reports, so the model reproduces it.
 */
export function resolveRef(raw: string, index: RefIndex): string | null {
  const cited = raw.trim();
  if (!cited) return null;
  if (index.byId.has(cited)) return cited;

  const cleaned = cited
    .replace(/^\.\//, "")
    .replace(/:\d+(-\d+)?$/, "")
    .toLowerCase();
  if (index.byId.has(cleaned)) return cleaned;
  if (index.byPath.has(cleaned)) return index.byPath.get(cleaned)!;
  if (index.byPathName.has(cleaned)) return index.byPathName.get(cleaned)!;

  const colon = cleaned.lastIndexOf(":");
  if (colon > 0) {
    const tail = cleaned.slice(colon + 1);
    if (index.byUniqueName.has(tail)) return index.byUniqueName.get(tail)!;
  }
  return index.byUniqueName.get(cleaned) ?? null;
}

/** Collect the citations in an answer that resolve to a real node. */
function extractRefs(answer: string, index: RefIndex): string[] {
  const refs = new Set<string>();
  let match: RegExpExecArray | null;
  REF_RE.lastIndex = 0;
  while ((match = REF_RE.exec(answer)) !== null) {
    const id = resolveRef(match[1], index);
    if (id) refs.add(id);
  }
  return [...refs];
}

/**
 * Replace `[[…]]` tokens with the node's name as inline code so prose reads
 * cleanly. An UNRESOLVED citation keeps its literal text rather than vanishing —
 * dropping it silently edits the model's sentence and hides that the citation
 * missed, which is exactly how this feature stayed broken unnoticed.
 */
function humanizeAnswer(answer: string, index: RefIndex): string {
  // FENCE-AWARE, and it has to be. `[[…]]` is also Mermaid's subroutine node
  // shape — `id[[Some Label]]` — so running the citation rewrite over the whole
  // message turned valid diagram syntax into `` id`Some Label` `` and the
  // diagram failed to parse. Citations only ever appear in prose, so code
  // fences are copied through untouched.
  return answer
    .split(/(```[\s\S]*?```|```[\s\S]*$)/g)
    .map((segment) =>
      segment.startsWith("```")
        ? segment
        : segment.replace(REF_RE, (_full, raw: string) => {
            const id = resolveRef(raw, index);
            const node = id ? index.byId.get(id) : undefined;
            return node ? `\`${node.name}\`` : `\`${raw.trim()}\``;
          }),
    )
    .join("");
}

/**
 * Models offered in the picker. The backend's ALLOWED_MODELS is deliberately
 * wider — it still accepts the 4.x ids so an existing CLAUDE_MODEL setting
 * keeps working — but only these are worth choosing for new questions.
 */
const MODELS = import.meta.env.VITE_CHAT_MODEL ? [
  { id: import.meta.env.VITE_CHAT_MODEL, label: import.meta.env.VITE_CHAT_MODEL, hint: "Configured model" },
] : [
  { id: "claude-opus-5", label: "Opus 5", hint: "Deepest reasoning" },
  { id: "claude-sonnet-5", label: "Sonnet 5", hint: "Balanced, faster" },
  { id: "claude-haiku-4-5-20251001", label: "Haiku 4.5", hint: "Quick lookups" },
];

/** Values understood by claude_agent.py:369-382 — do not invent new ones. */
const ROLES = [
  { id: "general", label: "General" },
  { id: "developer", label: "Developer" },
  { id: "business-analyst", label: "Business Analyst" },
];

/** Which project+stream the chat is scoped to, read from the URL/localStorage
 *  exactly as App.tsx resolves it. The chat must answer about the repository
 *  currently ON SCREEN — the earlier menu had independent project/stream pickers,
 *  which let the answer drift away from the graph the user was looking at. */
const API_BASE = import.meta.env.VITE_API_BASE ?? "";

/**
 * The on-screen rectangle of the graph canvas, tracked live.
 *
 * The expanded chat used fixed offsets — `top-[150px] right-[320px]` — which
 * encoded one particular window size. At any other resolution, zoom level or
 * sidebar width it either overlapped the INFO/FILES panel or left a gap.
 * Measuring the element the graph actually occupies is correct at every size,
 * and follows the sidebar if it is ever resized or hidden.
 */
function useGraphCanvasRect(active: boolean) {
  const [rect, setRect] = useState<DOMRect | null>(null);

  useEffect(() => {
    if (!active) return;
    const el = document.querySelector("[data-graph-canvas]");
    if (!el) return;

    const measure = () => setRect(el.getBoundingClientRect());
    measure();

    // ResizeObserver catches sidebar/panel changes; the scroll and resize
    // listeners catch the window itself moving the element.
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    ro.observe(document.documentElement);
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", measure, true);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", measure);
      window.removeEventListener("scroll", measure, true);
    };
  }, [active]);

  return rect;
}

function currentScope(): { project: string; stream: string } {
  const p = new URLSearchParams(window.location.search);
  return {
    project: p.get("project") || localStorage.getItem("prx-project") || "default",
    stream: p.get("stream") || localStorage.getItem("prx-stream") || "main",
  };
}

export default function InsightAdvisor({ accessToken }: { accessToken: string }) {
  const graph = useDashboardStore((s) => s.graph);
  // NOTE: no `searchEngine` subscription. The agent does its own retrieval, so
  // reading it here was an unused value AND a live subscription that re-rendered
  // the whole dock every time the search index changed.
  const nodesById = useDashboardStore((s) => s.nodesById);
  const refIndex = useMemo(() => buildRefIndex(nodesById), [nodesById]);
  const navigateToNode = useDashboardStore((s) => s.navigateToNode);
  const openCodeViewer = useDashboardStore((s) => s.openCodeViewer);

  const [open, setOpen] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const canvasRect = useGraphCanvasRect(open && expanded);
  const [showSettings, setShowSettings] = useState(false);
  // Chat history is its own panel, not a section buried at the bottom of
  // settings. It was invisible there: nothing on the header hinted the history
  // existed, and reaching it meant opening a panel about models and scope.
  const [showChats, setShowChats] = useState(false);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  // `markdownComponents` is memoised with no deps so the renderer identity stays
  // stable across tokens; a ref is how it reads the live streaming state without
  // rebuilding the whole component map on every delta. `loading` is state, so a
  // flip to false re-renders the list and the fence is drawn then.
  const streamingRef = useRef(false);
  useEffect(() => {
    streamingRef.current = loading;
  }, [loading]);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [status, setStatus] = useState<string | null>(null);

  /**
   * Server-side conversation id for this chat.
   *
   * The backend has had a complete per-user conversation store since the port
   * -- `conversations` + `conversation_messages` keyed by user_email, with
   * create/list/get/append/delete wired at /api/conversations
   * (main.py:511-605) -- and the dock called NONE of it. Messages lived only in
   * this component's state, so every reload, re-login or project switch threw
   * the whole history away while the tables sat empty.
   *
   * Kept in a ref, not state: it is written during a send and read by the next
   * one, and re-rendering the dock because an id was assigned would serve no
   * purpose.
   */
  const conversationIdRef = useRef<string | null>(null);
  const [conversations, setConversations] = useState<
    Array<{ id: string; title: string; updated_at?: string }>
  >([]);

  // Chat settings, persisted per browser. They map 1:1 onto fields ChatRequest
  // (backend/app/models.py:13) already accepts, so none of this needed new
  // backend surface — the dock simply never sent them.
  const [model, setModel] = useState(() => {
    const saved = localStorage.getItem("prx-chat-model");
    return MODELS.some((option) => option.id === saved) ? saved! : MODELS[0].id;
  });
  const [userRole, setUserRole] = useState(() => localStorage.getItem("prx-chat-role") ?? "general");
  const [reasoning, setReasoning] = useState(() => localStorage.getItem("prx-chat-reasoning") ?? "strict");
  const [diagrams, setDiagrams] = useState(() => localStorage.getItem("prx-chat-diagrams") ?? "mermaid");
  useEffect(() => { localStorage.setItem("prx-chat-model", model); }, [model]);
  useEffect(() => { localStorage.setItem("prx-chat-role", userRole); }, [userRole]);
  useEffect(() => { localStorage.setItem("prx-chat-reasoning", reasoning); }, [reasoning]);
  useEffect(() => { localStorage.setItem("prx-chat-diagrams", diagrams); }, [diagrams]);

  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  // Aborts the in-flight answer. A tool-using agent can spend a minute on a
  // question, so a mistyped or half-finished one used to cost that minute
  // before the composer accepted anything else.
  const abortRef = useRef<AbortController | null>(null);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages, loading, open]);

  useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);

  // Unmounting mid-stream would otherwise leave the agent running against a
  // reader nobody holds. Closing the dock deliberately does NOT abort: the
  // messages survive in state, so reopening shows the finished answer.
  useEffect(() => () => abortRef.current?.abort(), []);

  /** Overwrite the streaming assistant message in place. */
  const replaceLast = useCallback((msg: ChatMessage) => {
    setMessages((prev) => [...prev.slice(0, -1), msg]);
  }, []);

  const doExport = useCallback(
    async (format: "md" | "html") => {
      if (messages.length === 0) return;
      const now = new Date();
      const stamp = now.toISOString().slice(0, 10);
      const meta = {
        scope: graph?.project.name ?? "unknown",
        model: MODELS.find((m) => m.id === model)?.label ?? model,
        role: ROLES.find((r) => r.id === userRole)?.label ?? userRole,
        exportedAt: now.toISOString().replace("T", " ").slice(0, 19) + " UTC",
      };
      const name = exportFilename(meta.scope, format, stamp);
      if (format === "md") {
        downloadText(name, "text/markdown", buildMarkdown(messages, meta));
      } else {
        downloadText(name, "text/html", await buildHtml(messages, meta));
      }
    },
    [messages, graph, model, userRole],
  );

  const jumpToNode = useCallback(
    (id: string) => {
      const node = nodesById.get(id);
      if (!node) return;
      navigateToNode(id);
      if (node.filePath) openCodeViewer(id);
    },
    [nodesById, navigateToNode, openCodeViewer],
  );

  /**
   * Cancel the answer in flight, keeping whatever already streamed.
   *
   * Aborting the fetch closes the connection; Starlette notices the disconnect
   * at the generator's next yield and stops the agent server-side too, so this
   * is a real cancel rather than only hiding the output.
   */
  const stop = useCallback(() => {
    abortRef.current?.abort();
  }, []);

  // Restore the most recent conversation for this user and project.
  //
  // Scoped to the project because the chat is grounded in one codebase — the
  // dock's own subtitle says "Grounded in <project>" — so resuming a
  // conversation about a different repository would be worse than starting
  // fresh. Switching between the domain and structural views does not reach
  // here at all: the dock stays mounted, so that case was never the problem.
  useEffect(() => {
    if (!accessToken || accessToken === "__demo__") return;
    let cancelled = false;
    const { project, stream } = currentScope();
    (async () => {
      try {
        const res = await fetch(`${API_BASE}/api/conversations?limit=50`, {
          headers: { Authorization: `Bearer ${accessToken}` },
        });
        if (!res.ok) return;
        const body = await res.json();
        const forProject = (body.conversations ?? []).filter(
          (c: { project?: string; stream?: string }) =>
            c.project === project && c.stream === stream,
        );
        if (!cancelled) setConversations(forProject);
        const mine = forProject[0];
        if (!mine || cancelled) return;
        const full = await fetch(`${API_BASE}/api/conversations/${mine.id}`, {
          headers: { Authorization: `Bearer ${accessToken}` },
        });
        if (!full.ok || cancelled) return;
        const conv = await full.json();
        conversationIdRef.current = mine.id;
        const restored: ChatMessage[] = (conv.messages ?? [])
          .filter((m: { role?: string }) => m.role === "user" || m.role === "assistant")
          .map((m: { role: string; content: string }) => ({
            role: m.role as "user" | "assistant",
            content: m.content,
          }));
        if (restored.length) setMessages(restored);
      } catch {
        // History is an enhancement — a chat that cannot load its past must
        // still be usable for the next question.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [accessToken]);

  /** Create the conversation row on first use, then append both turns to it. */
  const persistTurn = useCallback(
    async (userText: string, assistantText: string) => {
      if (!accessToken || accessToken === "__demo__") return;
      const { project, stream } = currentScope();
      try {
        if (!conversationIdRef.current) {
          const created = await fetch(`${API_BASE}/api/conversations`, {
            method: "POST",
            headers: {
              "Content-Type": "application/json",
              Authorization: `Bearer ${accessToken}`,
            },
            body: JSON.stringify({ project, stream, model }),
          });
          if (!created.ok) return;
          conversationIdRef.current = (await created.json()).id ?? null;
        }
        if (!conversationIdRef.current) return;
        await fetch(
          `${API_BASE}/api/conversations/${conversationIdRef.current}/messages`,
          {
            method: "POST",
            headers: {
              "Content-Type": "application/json",
              Authorization: `Bearer ${accessToken}`,
            },
            body: JSON.stringify({
              messages: [
                { role: "user", content: userText },
                { role: "assistant", content: assistantText },
              ],
            }),
          },
        );
        // Refresh the list so a brand-new conversation appears under the
        // button without needing a reload.
        const listed = await fetch(`${API_BASE}/api/conversations?limit=50`, {
          headers: { Authorization: `Bearer ${accessToken}` },
        });
        if (listed.ok) {
          const body = await listed.json();
          setConversations(
            (body.conversations ?? []).filter(
              (c: { project?: string; stream?: string }) =>
                c.project === project && c.stream === stream,
            ),
          );
        }
      } catch {
        // Never let a storage failure surface as a chat failure: the answer is
        // already on screen and is the thing the user asked for.
      }
    },
    [accessToken, model],
  );

  /** Open a stored conversation, replacing whatever is on screen. */
  const loadConversation = useCallback(
    async (id: string) => {
      if (!accessToken) return;
      try {
        const res = await fetch(`${API_BASE}/api/conversations/${id}`, {
          headers: { Authorization: `Bearer ${accessToken}` },
        });
        if (!res.ok) return;
        const conv = await res.json();
        conversationIdRef.current = id;
        setMessages(
          (conv.messages ?? [])
            .filter((m: { role?: string }) => m.role === "user" || m.role === "assistant")
            .map((m: { role: string; content: string }) => ({
              role: m.role as "user" | "assistant",
              content: m.content,
            })),
        );
        setStatus(null);
        // Land the user in the conversation, not on the panel they opened it
        // from — picking a chat means "show me this chat".
        setShowSettings(false);
        setShowChats(false);
      } catch {
        // Same policy as restore: history is an enhancement, never a blocker.
      }
    },
    [accessToken],
  );

  /**
   * Start a fresh conversation.
   *
   * Three things, and only the first was happening: clear the transcript, DROP
   * THE SERVER-SIDE CONVERSATION ID -- otherwise the next answer is appended to
   * the previous conversation, which is the opposite of "new" -- and leave the
   * settings panel, because the button lives there and clearing a transcript
   * the user cannot see reads as the button doing nothing at all.
   */
  const newChat = useCallback(() => {
    conversationIdRef.current = null;
    setMessages([]);
    setStatus(null);
    setShowSettings(false);
    setShowChats(false);
  }, []);

  const send = useCallback(async () => {
    const question = input.trim();
    if (!question || loading) return;

    const history = messages
      .slice(-MAX_HISTORY_TURNS)
      .map((m) => ({ role: m.role, content: m.content }));

    setMessages((prev) => [...prev, { role: "user", content: question }]);
    setInput("");
    setLoading(true);

    // NOTE: no client-side `context` blob any more. The old code shipped a
    // locally assembled excerpt because the earlier viewer had no backend search.
    // Praxevia's agent runs its own tools against the pgvector index, so sending a
    // pre-chewed context both wasted tokens and capped the agent at whatever
    // the browser happened to have loaded.
    const { project, stream } = currentScope();

    // The assistant message is appended empty and filled by the stream.
    setMessages((prev) => [...prev, { role: "assistant", content: "" }]);
    setStatus("Thinking…");

    // Declared out here so the catch can keep a partial answer on abort.
    let answer = "";
    const controller = new AbortController();
    abortRef.current?.abort();  // defensive: never leave two streams running
    abortRef.current = controller;

    try {
      const res = await fetch(`${API_BASE}/api/chat`, {
        method: "POST",
        signal: controller.signal,
        headers: {
          "Content-Type": "application/json",
          ...(accessToken ? { Authorization: `Bearer ${accessToken}` } : {}),
        },
        // the backend's contract (backend/app/schemas.py). The dock previously sent
        // the earlier `{question, context, history}`, which this backend rejects with
        // 422 — verified against the running server. Every question failed.
        body: JSON.stringify({
          message: question,
          history,
          model,
          project,
          stream,
          diagram_mode: diagrams,
          reasoning_mode: reasoning,
          user_role: userRole,
        }),
      });

      if (res.status === 503) {
        replaceLast({
          role: "assistant", isError: true,
          content: "Insight Advisor needs an Anthropic API key. Set `ANTHROPIC_API_KEY` on the backend and restart it.",
        });
        return;
      }
      if (!res.ok || !res.body) {
        const detail = await res.json().catch(() => null);
        replaceLast({
          role: "assistant", isError: true,
          content: detail?.detail ?? `Insight Advisor request failed (${res.status}).`,
        });
        return;
      }

      // Server-Sent Events, not a JSON body. The previous code awaited
      // `res.json()` and would have hung on the stream even once the request
      // shape was right.
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";

      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });

        // Frames are separated by a blank line. Keep the trailing partial.
        const frames = buffer.split("\n\n");
        buffer = frames.pop() ?? "";

        for (const frame of frames) {
          const line = frame.split("\n").find((l) => l.startsWith("data:"));
          if (!line) continue; // `: keepalive` comments carry no data
          let ev: { type?: string; data?: Record<string, unknown> };
          try {
            ev = JSON.parse(line.slice(5).trim());
          } catch {
            continue;
          }

          if (ev.type === "content_delta") {
            answer += String(ev.data?.text ?? "");
            replaceLast({ role: "assistant", content: answer, refs: extractRefs(answer, refIndex) });
          } else if (ev.type === "tool_use") {
            setStatus(`Searching: ${String(ev.data?.tool_name ?? "index")}…`);
          } else if (ev.type === "tool_result") {
            setStatus(`Found ${String(ev.data?.count ?? 0)} results…`);
          } else if (ev.type === "error") {
            replaceLast({
              role: "assistant", isError: true,
              content: String(ev.data?.message ?? "The agent reported an error."),
            });
            return;
          }
        }
      }

      if (!answer.trim()) {
        replaceLast({ role: "assistant", isError: true, content: "_No answer returned._" });
      } else {
        // Persist AFTER the turn settles, so a partial stream never lands in
        // history as if it were the whole answer.
        void persistTurn(question, answer);
      }
    } catch (err) {
      // A cancel is not a failure. Keep the partial answer and say it was cut
      // short — throwing away what streamed would punish the user for stopping
      // a run that had already told them something useful.
      if ((err as Error)?.name === "AbortError") {
        replaceLast(
          answer.trim()
            ? { role: "assistant", content: `${answer}\n\n_Stopped._`,
                refs: extractRefs(answer, refIndex) }
            : { role: "assistant", content: "_Stopped before any answer._" },
        );
      } else {
        replaceLast({
          role: "assistant", isError: true,
          content: "Could not reach the backend. Is it still running?",
        });
      }
    } finally {
      // Only clear the handle if it is still ours: a follow-up question
      // installs its own controller, and clearing that one would make its
      // stop button do nothing.
      if (abortRef.current === controller) abortRef.current = null;
      setStatus(null);
      setLoading(false);
    }
  }, [input, loading, messages, refIndex, accessToken, model, userRole, reasoning, diagrams, replaceLast, persistTurn]);

  const onKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        void send();
      } else if (e.key === "Escape") {
        // Never cascade to the global Escape handler from inside the composer.
        e.stopPropagation();
        e.nativeEvent.stopImmediatePropagation();
        // Escape cancels the answer first and closes the dock second. Closing
        // mid-stream would leave the request running with nothing to show it,
        // and "get me out of this wrong answer" is the more urgent intent.
        if (loading) stop();
        else setOpen(false);
      }
    },
    [send, stop, loading],
  );

  const markdownComponents = useMemo(
    () => ({
      p: ({ children }: { children?: React.ReactNode }) => (
        <p className="mb-2 last:mb-0">{children}</p>
      ),
      strong: ({ children }: { children?: React.ReactNode }) => (
        <strong className="font-semibold text-text-primary">{children}</strong>
      ),
      a: ({ children, href }: { children?: React.ReactNode; href?: string }) => (
        <a
          href={href}
          target="_blank"
          rel="noreferrer"
          className="text-accent underline underline-offset-2"
        >
          {children}
        </a>
      ),
      code: ({ className, children }: { className?: string; children?: React.ReactNode }) => {
        const isBlock = className?.includes("language-");
        // A ```mermaid fence is a DIAGRAM, not a code sample. Until this
        // branch existed the model's diagram output was printed verbatim as
        // monospace text, which is what the Mermaid/Text toggle above is
        // supposed to choose between.
        if (className?.includes("language-mermaid")) {
          // While the reply streams, the fence is incomplete on every token and
          // mermaid.parse would reject it dozens of times before the last one
          // succeeds — a flicker of error states ending in a diagram. Show the
          // source until the message is finished, then draw it.
          if (streamingRef.current) {
            return (
              <code className="block bg-root/60 border border-border-subtle bevel-sm px-2 py-1.5 my-1.5 overflow-x-auto text-[11px] leading-relaxed font-mono">
                {children}
              </code>
            );
          }
          return (
            <Suspense fallback={<div className="my-1.5 text-[11px] text-text-muted">Drawing diagram…</div>}>
              <MermaidDiagram source={String(children ?? "").trimEnd()} />
            </Suspense>
          );
        }
        return isBlock ? (
          <code className="block bg-root/60 border border-border-subtle bevel-sm px-2 py-1.5 my-1.5 overflow-x-auto text-[11px] leading-relaxed font-mono">
            {children}
          </code>
        ) : (
          <code className="bg-elevated bevel-sm px-1 py-0.5 text-[11px] font-mono text-text-primary">
            {children}
          </code>
        );
      },
      ul: ({ children }: { children?: React.ReactNode }) => (
        <ul className="list-disc list-inside mb-2 space-y-1">{children}</ul>
      ),
      ol: ({ children }: { children?: React.ReactNode }) => (
        <ol className="list-decimal list-inside mb-2 space-y-1">{children}</ol>
      ),
    }),
    [],
  );

  // No server proxy in the static hosted demo — nothing to talk to.
  if (accessToken === "__demo__") return null;

  return (
    <div
      className={
        expanded && open
          ? "fixed z-40 flex flex-col"
          // bottom-16 leaves a row under the launcher for React Flow's
          // horizontal zoom controls, which previously overlapped it.
          : "fixed bottom-16 left-4 z-40 flex flex-col items-start"
      }
      // Expanded covers the graph canvas EXACTLY — never the header, the search
      // bar, or the INFO/FILES sidebar. Comparing an answer against the node on
      // screen is the point, so the sidebar must stay visible. Falls back to the
      // dock position until the first measurement lands.
      style={
        expanded && open && canvasRect
          ? {
              top: canvasRect.top,
              left: canvasRect.left,
              width: canvasRect.width,
              height: canvasRect.height,
            }
          : undefined
      }
    >
      {open && (
        <div
          className={`flex flex-col bevel border border-border-medium bg-surface bevel-elevate overflow-hidden animate-slide-up ${
            expanded
              ? "flex-1 min-h-0 w-full"
              : "mb-2 w-[92vw] max-w-[380px] h-[min(560px,70vh)]"
          }`}
        >
          {/* Header */}
          <div className="flex items-center gap-2 px-3 py-2.5 border-b border-border-subtle bg-elevated shrink-0">
            <BotIcon className="w-5 h-5 shrink-0 text-accent" />
            {/* The TITLE yields when space runs short, never the controls. */}
            <div className="flex flex-col min-w-0 overflow-hidden">
              <span className="font-heading text-sm text-text-primary leading-none tracking-wide truncate">
                Insight Advisor
              </span>
              <span className="text-[10px] text-text-muted truncate">
                {/* The scope is shown, never chosen here. The earlier menu had
                    independent PROJECT/STREAM pickers, which let the chat answer
                    about a repository other than the one drawn on screen. */}
                {graph?.project.name
                  ? `Grounded in ${graph.project.name}`
                  : "Ask about this codebase"}
              </span>
            </div>

            <button
              type="button"
              onClick={() => {
                // One panel at a time: the two overlay the same area, and
                // stacking them would bury whichever opened first.
                setShowChats((v) => !v);
                setShowSettings(false);
              }}
              aria-label="Chat history"
              aria-expanded={showChats}
              title="Chats"
              // Icon-only, and the same 7x7 box as the gear/expand/close
              // buttons beside it. A labelled button was wider than the header
              // had room for at the dock's width, so it clipped.
              className={`ml-auto w-7 h-7 shrink-0 flex items-center justify-center bevel-sm transition-colors ${
                showChats ? "text-accent bg-accent/10" : "text-text-muted hover:text-text-primary hover:bg-surface"
              }`}
            >
              <svg className="w-4 h-4" fill="none" stroke="currentColor" strokeWidth={2} viewBox="0 0 24 24">
                {/* A CLOSED bubble. The previous path's arcs never met on the
                    left, leaving an open edge that read as the icon being
                    cropped — it was drawn that way, not clipped by anything. */}
                <path strokeLinecap="round" strokeLinejoin="round" d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
                <path strokeLinecap="round" d="M8 8h8M8 12h5" />
              </svg>
            </button>

            <button
              type="button"
              onClick={() => {
                setShowSettings((v) => !v);
                setShowChats(false);
              }}
              aria-label="Chat settings"
              aria-expanded={showSettings}
              className={`w-7 h-7 shrink-0 flex items-center justify-center bevel-sm transition-colors ${
                showSettings ? "text-accent bg-accent/10" : "text-text-muted hover:text-text-primary hover:bg-surface"
              }`}
            >
              <svg className="w-4 h-4" fill="none" stroke="currentColor" strokeWidth={2} viewBox="0 0 24 24">
                <circle cx="12" cy="12" r="3" />
                <path strokeLinecap="round" d="M12 2v3m0 14v3M2 12h3m14 0h3M4.9 4.9l2.1 2.1m10 10l2.1 2.1M19.1 4.9L17 7m-10 10l-2.1 2.1" />
              </svg>
            </button>

            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              aria-label={expanded ? "Restore chat to dock" : "Expand chat"}
              className="w-7 h-7 shrink-0 flex items-center justify-center bevel-sm text-text-muted hover:text-text-primary hover:bg-surface transition-colors"
            >
              <svg className="w-4 h-4" fill="none" stroke="currentColor" strokeWidth={2} viewBox="0 0 24 24">
                {expanded ? (
                  <path strokeLinecap="round" strokeLinejoin="round" d="M9 9H4m5 0V4m6 5h5m-5 0V4M9 15H4m5 0v5m6-5h5m-5 0v5" />
                ) : (
                  <path strokeLinecap="round" strokeLinejoin="round" d="M4 9V4h5M20 9V4h-5M4 15v5h5m11-5v5h-5" />
                )}
              </svg>
            </button>

            <button
              type="button"
              onClick={() => setOpen(false)}
              aria-label="Close Insight Advisor"
              className="w-7 h-7 shrink-0 flex items-center justify-center bevel-sm text-text-muted hover:text-text-primary hover:bg-surface transition-colors"
            >
              <svg
                className="w-4 h-4"
                fill="none"
                stroke="currentColor"
                strokeWidth={2}
                viewBox="0 0 24 24"
              >
                <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          </div>

          {/* Settings — the controls the earlier UI kept in its left menu, folded into the
              dock so the default view stays as uncluttered as it was. Every one
              of these maps onto a field ChatRequest already accepted; the dock
              simply never sent them. */}
          {showSettings && (
            <div className="shrink-0 border-b border-border-subtle bg-root/40 px-3 py-3 space-y-3 text-xs">
              <div>
                <label className="block text-[10px] uppercase tracking-[0.1em] text-text-muted mb-1">
                  Scope
                </label>
                <div className="flex items-center justify-between bevel-sm border border-border-subtle px-2 py-1.5">
                  <span className="font-mono text-text-secondary truncate">
                    {graph?.project.name ?? "—"}
                  </span>
                  <span className="text-[9px] uppercase tracking-wider text-text-muted shrink-0 ml-2">
                    follows graph
                  </span>
                </div>
              </div>

              <div>
                <label htmlFor="prx-chat-model" className="block text-[10px] uppercase tracking-[0.1em] text-text-muted mb-1">
                  Model
                </label>
                <select
                  id="prx-chat-model"
                  value={model}
                  onChange={(e) => setModel(e.target.value)}
                  className="w-full bevel-sm border border-border-subtle bg-elevated px-2 py-1.5 text-text-primary focus:outline-none focus:border-accent"
                >
                  {MODELS.map((m) => (
                    <option key={m.id} value={m.id}>{`${m.label} — ${m.hint}`}</option>
                  ))}
                </select>
              </div>

              <div>
                <label htmlFor="prx-chat-role" className="block text-[10px] uppercase tracking-[0.1em] text-text-muted mb-1">
                  My role
                </label>
                <select
                  id="prx-chat-role"
                  value={userRole}
                  onChange={(e) => setUserRole(e.target.value)}
                  className="w-full bevel-sm border border-border-subtle bg-elevated px-2 py-1.5 text-text-primary focus:outline-none focus:border-accent"
                >
                  {ROLES.map((r) => (
                    <option key={r.id} value={r.id}>{r.label}</option>
                  ))}
                </select>
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <span className="block text-[10px] uppercase tracking-[0.1em] text-text-muted mb-1">
                    Reasoning
                  </span>
                  <div className="flex">
                    {(["strict", "creative"] as const).map((v) => (
                      <button
                        key={v}
                        type="button"
                        onClick={() => setReasoning(v)}
                        className={`flex-1 py-1.5 capitalize bevel-sm border transition-colors ${
                          reasoning === v
                            ? "bg-accent/15 text-accent border-accent/40"
                            : "text-text-muted border-border-subtle hover:text-text-primary"
                        }`}
                      >
                        {v}
                      </button>
                    ))}
                  </div>
                </div>
                <div>
                  <span className="block text-[10px] uppercase tracking-[0.1em] text-text-muted mb-1">
                    Diagrams
                  </span>
                  <div className="flex">
                    {(["mermaid", "text"] as const).map((v) => (
                      <button
                        key={v}
                        type="button"
                        onClick={() => setDiagrams(v)}
                        className={`flex-1 py-1.5 capitalize bevel-sm border transition-colors ${
                          diagrams === v
                            ? "bg-accent/15 text-accent border-accent/40"
                            : "text-text-muted border-border-subtle hover:text-text-primary"
                        }`}
                      >
                        {v}
                      </button>
                    ))}
                  </div>
                </div>
              </div>

              <div>
                <span className="block text-[10px] uppercase tracking-[0.1em] text-text-muted mb-1">
                  Export transcript
                </span>
                <div className="grid grid-cols-2 gap-2">
                  {(["md", "html"] as const).map((f) => (
                    <button
                      key={f}
                      type="button"
                      disabled={messages.length === 0}
                      onClick={() => void doExport(f)}
                      className="py-1.5 uppercase tracking-wider bevel-sm border border-dashed border-border-subtle text-text-muted hover:text-accent hover:border-accent/40 transition-colors disabled:opacity-35 disabled:cursor-not-allowed disabled:hover:text-text-muted disabled:hover:border-border-subtle"
                    >
                      {f}
                    </button>
                  ))}
                </div>
              </div>


            </div>
          )}

          {/* Chat history. Sits in the same slot as the settings panel and
              flexes to the dock's height, so it scrolls in the small dock and
              simply shows more rows when the chat is expanded over the graph —
              one component, both states, no duplicated markup. */}
          {showChats && (
            <div className="border-b border-border-subtle bg-surface/60 px-3 py-2 flex flex-col min-h-0 max-h-[45vh]">
              <div className="flex items-center justify-between mb-1.5">
                <span className="text-[10px] uppercase tracking-[0.1em] text-text-muted">
                  Chats
                </span>
                <button
                  type="button"
                  onClick={newChat}
                  className="text-[11px] px-2 py-1 bevel-sm border border-dashed border-border-subtle text-text-muted hover:text-accent hover:border-accent/40 transition-colors"
                >
                  + New chat
                </button>
              </div>
              {conversations.length === 0 ? (
                <p className="text-[11px] text-text-muted py-2">
                  No previous chats for this project yet.
                </p>
              ) : (
                <div className="flex-1 min-h-0 overflow-y-auto flex flex-col gap-0.5 pr-0.5">
                  {conversations.map((c) => (
                    <button
                      key={c.id}
                      type="button"
                      onClick={() => loadConversation(c.id)}
                      title={c.title}
                      className={`w-full text-left px-2 py-1.5 text-[11px] bevel-sm truncate transition-colors ${
                        conversationIdRef.current === c.id
                          ? "bg-accent/15 text-accent"
                          : "text-text-muted hover:text-text-primary hover:bg-surface"
                      }`}
                    >
                      {c.title || "Untitled chat"}
                    </button>
                  ))}
                </div>
              )}
            </div>
          )}

          {/* Messages */}
          <div ref={scrollRef} className="flex-1 min-h-0 overflow-y-auto px-3 py-3 space-y-3">
            {messages.length === 0 && (
              <div className="text-xs text-text-muted leading-relaxed">
                Ask me anything about this codebase — where something is defined,
                how a feature works, or which files handle a concern. I answer
                from the analyzed index and link you straight to the code.
              </div>
            )}

            {messages.map((msg, i) =>
              msg.role === "user" ? (
                <div key={i} className="flex justify-end">
                  <div className="max-w-[85%] bevel-sm bg-accent/15 text-text-primary px-3 py-2 text-sm whitespace-pre-wrap break-words">
                    {msg.content}
                  </div>
                </div>
              ) : (
                <div key={i} className="flex flex-col gap-1.5">
                  <div
                    className={`max-w-[92%] bevel-sm px-3 py-2 text-sm break-words ${
                      msg.isError
                        ? "bg-red-900/25 border border-red-700/50 text-red-200"
                        : "bg-elevated text-text-secondary"
                    }`}
                  >
                    <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
                      {humanizeAnswer(msg.content, refIndex)}
                    </ReactMarkdown>
                  </div>
                  {msg.refs && msg.refs.length > 0 && (
                    <div className="flex flex-wrap gap-1.5 pl-0.5">
                      {msg.refs.map((id) => {
                        const node = nodesById.get(id);
                        if (!node) return null;
                        return (
                          <button
                            key={id}
                            type="button"
                            // A deliberate test hook. e2e/chat_citations.py used
                            // to find chips by "a button whose title contains a
                            // slash", which also matched the project switcher —
                            // the assertion passed while counting the wrong thing.
                            data-ref-chip={id}
                            onClick={() => jumpToNode(id)}
                            title={
                              node.filePath
                                ? `${node.filePath}${
                                    node.lineRange
                                      ? `:${node.lineRange[0]}-${node.lineRange[1]}`
                                      : ""
                                  }`
                                : node.name
                            }
                            className="inline-flex items-center gap-1 max-w-[220px] px-2 py-0.5 bevel-sm border border-border-medium bg-surface text-[11px] text-text-secondary hover:text-accent hover:border-accent/60 transition-colors"
                          >
                            <svg
                              className="w-3 h-3 shrink-0"
                              fill="none"
                              stroke="currentColor"
                              strokeWidth={2}
                              viewBox="0 0 24 24"
                            >
                              <path
                                strokeLinecap="round"
                                strokeLinejoin="round"
                                d="M13 7h8m0 0v8m0-8l-8 8-4-4-6 6"
                              />
                            </svg>
                            <span className="truncate">{node.name}</span>
                          </button>
                        );
                      })}
                    </div>
                  )}
                </div>
              ),
            )}

            {loading && (
              <div className="flex items-center gap-1.5 text-text-muted text-sm pl-1">
                <span className="w-1.5 h-1.5 rounded-full bg-text-muted animate-bounce [animation-delay:-0.3s]" />
                <span className="w-1.5 h-1.5 rounded-full bg-text-muted animate-bounce [animation-delay:-0.15s]" />
                <span className="w-1.5 h-1.5 rounded-full bg-text-muted animate-bounce" />
                {/* What the agent is actually doing. A long silence during tool
                    calls otherwise looks like a hang. */}
                {status && <span className="ml-1 text-xs">{status}</span>}
              </div>
            )}
          </div>

          {/* Composer */}
          <div className="shrink-0 border-t border-border-subtle bg-surface p-2">
            <div className="flex items-end gap-2 bevel-sm border border-border-subtle bg-root/40 px-2 py-1.5 focus-within:border-accent/60 transition-colors">
              <textarea
                ref={inputRef}
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={onKeyDown}
                rows={1}
                placeholder={
                  graph ? "Ask about this codebase…" : "Loading codebase index…"
                }
                disabled={!graph}
                className="flex-1 resize-none bg-transparent text-sm text-text-primary placeholder:text-text-muted focus:outline-none max-h-28 leading-relaxed disabled:opacity-60"
              />
              {/* One button, two jobs. While an answer streams it becomes
                  Stop — the agent can spend a minute on tool calls, and the
                  composer is otherwise blocked for that whole minute after a
                  mistyped question. The arrow/square swap is the standard
                  affordance, so the state is readable without a label. */}
              <button
                type="button"
                onClick={() => (loading ? stop() : void send())}
                disabled={loading ? false : !input.trim() || !graph}
                aria-label={loading ? "Stop generating" : "Send"}
                title={loading ? "Stop generating (Esc)" : "Send (Enter)"}
                className="w-8 h-8 shrink-0 flex items-center justify-center bevel-sm bg-accent/20 text-accent hover:bg-accent/30 disabled:opacity-40 disabled:hover:bg-accent/20 transition-colors"
              >
                {loading ? (
                  <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
                    {/* Deliberately not `rounded`: the design language cuts
                        corners rather than rounding them. */}
                    <rect x="7" y="7" width="10" height="10" />
                  </svg>
                ) : (
                  <svg
                    className="w-4 h-4"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth={2}
                    viewBox="0 0 24 24"
                  >
                    <path strokeLinecap="round" strokeLinejoin="round" d="M5 12h14m0 0l-6-6m6 6l-6 6" />
                  </svg>
                )}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Launcher */}
      {/* The launcher is redundant once the chat owns the canvas, and it
          overlapped the composer. */}
      {!(open && expanded) && (
      <button
        type="button"
        onClick={() => setOpen((prev) => !prev)}
        aria-label={open ? "Hide Insight Advisor" : "Open Insight Advisor"}
        aria-expanded={open}
        // `whitespace-nowrap` + `w-max`: the label is the button's width, so
        // "Insight Advisor" cannot be clipped at any font size.
        className={`flex items-center gap-2 h-11 pl-3 pr-4 w-max whitespace-nowrap bevel-sm border transition-colors ${
          open
            ? "bg-elevated border-border-medium text-text-secondary"
            : "bg-accent/20 border-accent/50 text-accent hover:bg-accent/30"
        }`}
      >
        <BotIcon className="w-5 h-5" />
        <span className="text-sm font-semibold tracking-wide">Insight Advisor</span>
      </button>
      )}
    </div>
  );
}

function BotIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      viewBox="0 0 24 24"
      aria-hidden="true"
    >
      <rect x="4" y="8" width="16" height="11" rx="2.5" />
      <path strokeLinecap="round" d="M12 8V4m0 0h-1.5m1.5 0h1.5" />
      <circle cx="9" cy="13" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="15" cy="13" r="1.1" fill="currentColor" stroke="none" />
      <path strokeLinecap="round" d="M9.5 16.5h5" />
    </svg>
  );
}
