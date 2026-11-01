import { useEffect, useState } from "react";
import { HaloLogo } from "./Icons";
import { rest } from "../rest";
import type { AuthStatus, User } from "../types";

type Props = {
  status: AuthStatus;
  onAuthed: (user: User) => void;
};

type Mode = "login" | "signup" | "setup";
type FieldKey = "username" | "password" | "question" | "answer" | "confirm";
type Errors = Partial<Record<FieldKey, string>>;

const RECOVERY_QUESTIONS = [
  "What was the name of your first pet?",
  "What city were you born in?",
  "What was the name of your first school?",
  "What was your favourite teacher's name?",
  "What is your mother's maiden name?",
];
const CUSTOM_QUESTION = "__custom__";

const inputCls =
  "w-full rounded-lg border bg-bg px-3 py-2 text-sm text-ink outline-none focus:border-accent";
const fieldCls = (bad: boolean) => `${inputCls} ${bad ? "border-red-500" : "border-line"}`;

function FieldError({ msg }: { msg?: string }) {
  if (!msg) return null;
  return (
    <p role="alert" className="mt-1 text-xs text-red-500">
      {msg}
    </p>
  );
}

function Labelled({
  label,
  error,
  children,
}: {
  label: string;
  error?: string;
  children: React.ReactNode;
}) {
  return (
    <label className="mb-3 block">
      <span className="mb-1 block text-xs font-medium text-muted">{label}</span>
      {children}
      <FieldError msg={error} />
    </label>
  );
}

export default function AuthGate({ status, onAuthed }: Props) {
  const mode: Mode = status.needs_setup ? "setup" : "login";
  const [view, setView] = useState<Mode>(mode);
  const [step, setStep] = useState<1 | 2>(1);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [question, setQuestion] = useState(RECOVERY_QUESTIONS[0]);
  const [customQuestion, setCustomQuestion] = useState("");
  const [answer, setAnswer] = useState("");
  const [confirm, setConfirm] = useState("");
  const [errors, setErrors] = useState<Errors>({});
  const [alert, setAlert] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [version, setVersion] = useState("");
  useEffect(() => {
    rest
      .health()
      .then((h) => setVersion(h.version))
      .catch(() => setVersion(""));
  }, []);

  const isRegister = view === "signup" || view === "setup";
  const finalQuestion = question === CUSTOM_QUESTION ? customQuestion.trim() : question;

  const validField = (key: FieldKey): string | null => {
    switch (key) {
      case "username": {
        const u = username.trim().toLowerCase();
        if (!u) return "username is required";
        if (!/^[a-z0-9._-]{3,32}$/.test(u))
          return "3-32 characters: a-z, 0-9, '.', '_', '-'";
        return null;
      }
      case "password":
        if (!password) return "password is required";
        if (password.length < 8) return "password must be at least 8 characters";
        return null;
      case "question":
        if (question === CUSTOM_QUESTION && !customQuestion.trim())
          return "enter a recovery question";
        if (finalQuestion.length > 200) return "question is too long (max 200 characters)";
        return null;
      case "answer":
        if (answer.trim().length < 3) return "answer must be at least 3 characters";
        return null;
      case "confirm":
        if (confirm.trim() !== answer.trim()) return "answers do not match";
        return null;
    }
  };

  const validate = (keys: FieldKey[]): Errors => {
    const e: Errors = {};
    for (const k of keys) {
      const msg = validField(k);
      if (msg) e[k] = msg;
    }
    return e;
  };

  const blur = (key: FieldKey) => (e: React.FocusEvent<HTMLInputElement>) => {
    if (!e.target.value && Object.keys(errors).length === 0) return;
    setErrors((prev) => ({ ...prev, [key]: validField(key) ?? undefined }));
  };

  const serverFieldError = (msg: string, currentStep: 1 | 2): Errors => {
    if (/username/.test(msg)) return { username: msg };
    if (/password/.test(msg)) return { password: msg };
    if (/question/.test(msg)) return currentStep === 2 ? { question: msg } : {};
    if (/answer/.test(msg)) return currentStep === 2 ? { answer: msg } : {};
    return {};
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (busy) return;
    setAlert(null);
    const keys: FieldKey[] =
      !isRegister || step === 1
        ? ["username", "password"]
        : ["question", "answer", "confirm"];
    const errs = validate(keys);
    setErrors(errs);
    if (Object.keys(errs).length > 0) return;
    setBusy(true);
    try {
      if (!isRegister) {
        onAuthed(await rest.login(username.trim(), password));
      } else if (step === 1) {
        setStep(2);
      } else {
        const call = view === "setup" ? rest.setup : rest.signup;
        onAuthed(
          await call(username.trim(), password, {
            recovery_question: finalQuestion,
            recovery_answer: answer.trim(),
          }),
        );
      }
    } catch (err) {
      const msg = (err as Error).message;
      if (isRegister && /taken/i.test(msg)) {
        setStep(1);
        setErrors({ username: msg });
      } else {
        const mapped = serverFieldError(msg, step);
        if (Object.keys(mapped).length > 0) setErrors(mapped);
        else setAlert(msg);
      }
    } finally {
      setBusy(false);
    }
  };

  const switchView = (v: Mode) => {
    setView(v);
    setStep(1);
    setErrors({});
    setAlert(null);
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
            <h1 className="text-lg font-semibold text-ink">
              HaloTalk
              {version && (
                <span data-testid="app-version" className="ml-1.5 align-middle text-xs text-muted">
                  v{version}
                </span>
              )}
            </h1>
            <p className="text-xs text-muted">
              {view === "setup"
                ? step === 1
                  ? "Create the administrator account"
                  : "Secure the account with a recovery question"
                : view === "signup"
                  ? step === 1
                    ? "Create your account"
                    : "Secure your account with a recovery question"
                  : "Sign in to continue"}
            </p>
          </div>
        </div>

        {alert && (
          <div
            role="alert"
            className="mb-3 rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-xs text-red-500"
          >
            {alert}
          </div>
        )}

        {(!isRegister || step === 1) && (
          <>
            <Labelled label="Username" error={errors.username}>
              <input
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                onBlur={blur("username")}
                autoFocus
                autoComplete="username"
                aria-invalid={!!errors.username}
                className={fieldCls(!!errors.username)}
              />
            </Labelled>
            <Labelled label="Password" error={errors.password}>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                onBlur={blur("password")}
                autoComplete={view === "login" ? "current-password" : "new-password"}
                aria-invalid={!!errors.password}
                className={fieldCls(!!errors.password)}
              />
            </Labelled>
            {view === "login" && (
              <p className="-mt-1 mb-3 text-xs text-muted">
                <span
                  title="Coming soon: reset your password by answering your recovery question"
                  className="cursor-help border-b border-dotted border-muted"
                >
                  Forgot password?
                </span>
              </p>
            )}
          </>
        )}

        {isRegister && step === 2 && (
          <>
            <div className="mb-3 text-xs font-medium text-muted">Step 2 of 2 · recovery</div>
            <Labelled label="Recovery question" error={errors.question}>
              <select
                value={question}
                onChange={(e) => setQuestion(e.target.value)}
                className={fieldCls(!!errors.question)}
              >
                {RECOVERY_QUESTIONS.map((q) => (
                  <option key={q} value={q}>
                    {q}
                  </option>
                ))}
                <option value={CUSTOM_QUESTION}>Write your own…</option>
              </select>
            </Labelled>
            {question === CUSTOM_QUESTION && (
              <Labelled label="Your question" error={errors.question}>
                <input
                  value={customQuestion}
                  onChange={(e) => setCustomQuestion(e.target.value)}
                  onBlur={blur("question")}
                  maxLength={200}
                  autoFocus
                  aria-invalid={!!errors.question}
                  className={fieldCls(!!errors.question)}
                />
              </Labelled>
            )}
            <Labelled label="Your answer" error={errors.answer}>
              <input
                value={answer}
                onChange={(e) => setAnswer(e.target.value)}
                onBlur={blur("answer")}
                autoComplete="off"
                autoFocus={question !== CUSTOM_QUESTION}
                aria-invalid={!!errors.answer}
                className={fieldCls(!!errors.answer)}
              />
            </Labelled>
            <Labelled label="Confirm answer" error={errors.confirm}>
              <input
                value={confirm}
                onChange={(e) => setConfirm(e.target.value)}
                onBlur={blur("confirm")}
                autoComplete="off"
                aria-invalid={!!errors.confirm}
                className={fieldCls(!!errors.confirm)}
              />
            </Labelled>
            <p className="mb-3 text-xs text-muted">
              You will need this answer to regain access if you forget your password.
            </p>
          </>
        )}

        <div className="flex gap-2">
          {isRegister && step === 2 && (
            <button
              type="button"
              onClick={() => {
                setStep(1);
                setErrors({});
                setAlert(null);
              }}
              className="rounded-lg border border-line px-4 py-2 text-sm font-medium text-muted hover:text-ink"
            >
              Back
            </button>
          )}
          <button
            type="submit"
            disabled={busy}
            className="flex-1 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-bg transition-opacity disabled:opacity-50"
          >
            {busy
              ? "Please wait…"
              : !isRegister
                ? "Sign in"
                : step === 1
                  ? "Continue"
                  : view === "setup"
                    ? "Create admin"
                    : "Create account"}
          </button>
        </div>

        {view !== "setup" && (
          <div className="mt-4 flex justify-between text-xs text-muted">
            {status.open_signup && view !== "signup" ? (
              <button
                type="button"
                className="hover:text-accent"
                onClick={() => switchView("signup")}
              >
                Create an account
              </button>
            ) : view === "signup" ? (
              <button
                type="button"
                className="hover:text-accent"
                onClick={() => switchView("login")}
              >
                Sign in instead
              </button>
            ) : (
              <span />
            )}
          </div>
        )}
      </form>
    </div>
  );
}
