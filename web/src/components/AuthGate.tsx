import { useState } from "react";
import { HaloLogo } from "./Icons";
import { rest } from "../rest";
import type { AuthStatus, User } from "../types";

type Props = {
  status: AuthStatus;
  onAuthed: (user: User) => void;
};

type Mode = "login" | "signup" | "setup";

export default function AuthGate({ status, onAuthed }: Props) {
  const mode: Mode = status.needs_setup ? "setup" : "login";
  const [view, setView] = useState<Mode>(mode);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const call =
        view === "setup" ? rest.setup : view === "signup" ? rest.signup : rest.login;
      onAuthed(await call(username.trim(), password));
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full items-center justify-center bg-bg px-4">
      <form
        onSubmit={(e) => void submit(e)}
        className="w-full max-w-sm rounded-2xl border border-line bg-surface p-6 shadow-sm"
      >
        <div className="mb-5 flex items-center gap-2.5">
          <HaloLogo className="h-8 w-8" />
          <div>
            <h1 className="text-lg font-semibold text-ink">HaloTalk</h1>
            <p className="text-xs text-muted">
              {view === "setup"
                ? "Create the administrator account"
                : view === "signup"
                  ? "Create your account"
                  : "Sign in to continue"}
            </p>
          </div>
        </div>

        <label className="mb-3 block">
          <span className="mb-1 block text-xs font-medium text-muted">Username</span>
          <input
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            autoFocus
            autoComplete="username"
            className="w-full rounded-lg border border-line bg-bg px-3 py-2 text-sm text-ink outline-none focus:border-accent"
          />
        </label>
        <label className="mb-4 block">
          <span className="mb-1 block text-xs font-medium text-muted">Password</span>
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete={view === "login" ? "current-password" : "new-password"}
            className="w-full rounded-lg border border-line bg-bg px-3 py-2 text-sm text-ink outline-none focus:border-accent"
          />
        </label>

        {error && <p className="mb-3 text-xs text-red-500">{error}</p>}

        <button
          type="submit"
          disabled={busy || !username.trim() || !password}
          className="w-full rounded-lg bg-accent px-4 py-2 text-sm font-medium text-bg transition-opacity disabled:opacity-50"
        >
          {busy ? "Please wait…" : view === "setup" ? "Create admin" : view === "signup" ? "Sign up" : "Sign in"}
        </button>

        {view !== "setup" && (
          <div className="mt-4 flex justify-between text-xs text-muted">
            {status.open_signup ? (
              view === "login" ? (
                <button type="button" className="hover:text-accent" onClick={() => setView("signup")}>
                  Create an account
                </button>
              ) : (
                <button type="button" className="hover:text-accent" onClick={() => setView("login")}>
                  Sign in instead
                </button>
              )
            ) : (
              <span />
            )}
          </div>
        )}
      </form>
    </div>
  );
}
