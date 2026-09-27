import { useState } from "react";

interface TokenGateProps {
  onTokenValid: (token: string) => void;
}

const API_BASE = import.meta.env.VITE_API_BASE ?? "";

/**
 * Sign-in gate —
 *
 * Upstream this validated a one-time process token by fetching the graph
 * endpoint and reading `res.ok`, which conflated "bad token" with "bad graph"
 * and with "no graph built yet". Here validation hits a dedicated
 * endpoint that answers exactly one question: is this identity known?
 *
 * Two ways in, because the backend accepts both:
 *   - username + password  -> POST /api/login, returns a session token
 *   - an API key           -> used directly as a Bearer token
 *
 * This is a development gate. The production surface is Praxevia's Entra ID / MSAL
 * flow, which arrives with the shell in
 */
export default function TokenGate({ onTokenValid }: TokenGateProps) {
  const [mode, setMode] = useState<"password" | "token">("password");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [token, setToken] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  /** Ask the backend whether this bearer is accepted. */
  const validate = async (bearer: string): Promise<boolean> => {
    const res = await fetch(`${API_BASE}/api/graph/projects`, {
      headers: { Authorization: `Bearer ${bearer}` },
    });
    return res.ok;
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setError(null);

    try {
      if (mode === "token") {
        const bearer = token.trim();
        if (!bearer) return;
        if (await validate(bearer)) {
          onTokenValid(bearer);
        } else {
          setError("That key was rejected.");
        }
        return;
      }

      const res = await fetch(`${API_BASE}/api/login`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        // Trim BOTH. Trimming only the username means a trailing space picked up
        // from a paste reports "incorrect password", which sends the user hunting
        // for a credential problem that does not exist.
        body: JSON.stringify({ username: username.trim(), password: password.trim() }),
      });
      if (res.status === 401) {
        setError("Incorrect username or password.");
        return;
      }
      if (!res.ok) {
        setError(`Backend returned ${res.status}.`);
        return;
      }
      const body = await res.json();
      if (body?.token) {
        onTokenValid(body.token);
      } else {
        setError("The backend returned no token.");
      }
    } catch (err) {
      setError(
        `Could not reach the backend: ${err instanceof Error ? err.message : String(err)}`,
      );
    } finally {
      setLoading(false);
    }
  };

  const field =
    "w-full px-4 py-3 bg-elevated border border-border-subtle bevel-sm text-text-primary " +
    "placeholder:text-text-muted/50 text-sm focus:outline-none focus:border-accent transition-colors";

  return (
    <div className="h-screen w-screen flex items-center justify-center bg-root">
      <div className="w-full max-w-md px-8 py-10 bg-surface border border-border-subtle bevel-sm bevel-elevate">
        <h1 className="prx-heading text-2xl text-text-primary text-center mb-2">
          Praxevia Explorer
        </h1>
        <p className="text-text-muted text-sm text-center mb-8">Sign in to continue</p>

        <div className="flex gap-2 mb-6 text-xs uppercase tracking-[0.1em]">
          {(["password", "token"] as const).map((m) => (
            <button
              key={m}
              type="button"
              onClick={() => {
                setMode(m);
                setError(null);
              }}
              className={`flex-1 py-2 bevel-sm transition-colors ${
                mode === m
                  ? "bg-accent/15 text-accent border border-accent/40"
                  : "text-text-muted border border-border-subtle hover:text-text-primary"
              }`}
            >
              {m === "password" ? "Password" : "API key"}
            </button>
          ))}
        </div>

        <form onSubmit={handleSubmit} className="flex flex-col gap-4">
          {mode === "password" ? (
            <>
              <input
                type="text"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder="Username"
                autoComplete="username"
                autoFocus
                className={field}
              />
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="Password"
                autoComplete="current-password"
                className={field}
              />
            </>
          ) : (
            <input
              type="password"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder="API key"
              autoFocus
              className={`${field} font-mono`}
            />
          )}

          {error && <p className="text-red-400 text-sm">{error}</p>}

          <button
            type="submit"
            disabled={loading}
            className="w-full py-3 bg-cta text-cta-fg font-medium uppercase tracking-[0.04em] bevel-sm transition-colors hover:bg-cta-hover disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {loading ? "Signing in…" : "Sign in"}
          </button>
        </form>
      </div>
    </div>
  );
}
