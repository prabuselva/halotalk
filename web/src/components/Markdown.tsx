import hljs from "highlight.js/lib/common";
import { useEffect, useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { IconChevronDown, IconDownload } from "./Icons";

// KaTeX/mermaid load on demand so they stay out of the main chunk.
let remarkMathMod: unknown = null;
let rehypeKatexMod: unknown = null;

function maybeMath(s: string): boolean {
  const stripped = s.replace(/```[\s\S]*?```/g, "").replace(/`[^`\n]+`/g, "");
  return /\$\$[\s\S]+?\$\$/.test(stripped) || /\$[^$\n]+\$/.test(stripped);
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]!);
}

// Structured/data languages get a collapsed-by-default view once the block
// is complete (long JSON/HTML dumps shouldn't push the chat around).
const STRUCT_LANGS = new Set([
  "json", "jsonl", "html", "xml", "yaml", "yml", "toml", "ini", "csv", "sql",
]);
const COLLAPSE_MIN_LINES = 14;

// Language → file extension for downloads when the fence carries no filename.
const LANG_EXT: Record<string, string> = {
  rust: "rs", python: "py", javascript: "js", typescript: "ts", bash: "sh",
  shell: "sh", yaml: "yml", markdown: "md", plaintext: "txt", "c++": "cpp",
  csharp: "cs", golang: "go", dockerfile: "dockerfile",
};

// Fence info conventions seen in LLM output: ```rust main.rs, ```rust:main.rs,
// ```main.rs, ```Cargo.toml — extract the filename if present.
const FILENAME_TOKEN = /^[\w+.-]+\.[A-Za-z][A-Za-z0-9]*$/;

export function parseFence(
  className: string | undefined,
  meta: unknown,
): { lang?: string; filename?: string } {
  const raw = (className ?? "").replace(/^language-/, "");
  const tokens = [...raw.split(/[\s:]+/), ...String(meta ?? "").split(/\s+/)].filter(Boolean);
  let lang: string | undefined;
  let filename: string | undefined;
  const looksLikeFilename = (t: string) =>
    FILENAME_TOKEN.test(t) && !hljs.getLanguage(t);
  const base = (t: string) => t.split(/[/\\]/).pop() ?? t;
  tokens.forEach((t, i) => {
    if (looksLikeFilename(t)) {
      if (!filename) {
        filename = base(t).replace(/^["']|["']$/g, "");
        if (i === 0 && !lang) {
          const ext = filename.slice(filename.lastIndexOf(".") + 1).toLowerCase();
          if (hljs.getLanguage(ext)) lang = ext;
        }
      }
    } else if (i === 0 && !lang) {
      lang = t;
    }
  });
  return { lang, filename };
}

function CodeBlock({
  code,
  lang,
  filename,
  streaming,
}: {
  code: string;
  lang?: string;
  filename?: string;
  streaming?: boolean;
}) {
  const html = useMemo(() => {
    try {
      return lang && hljs.getLanguage(lang)
        ? hljs.highlight(code, { language: lang }).value
        : hljs.highlightAuto(code).value;
    } catch {
      return escapeHtml(code);
    }
  }, [code, lang]);
  const [copied, setCopied] = useState(false);
  const [userOpen, setUserOpen] = useState<boolean | null>(null);
  const collapsible =
    !!lang && STRUCT_LANGS.has(lang.toLowerCase()) && code.split("\n").length > COLLAPSE_MIN_LINES;
  const open = userOpen ?? !(collapsible && !streaming);
  const lineCount = code.split("\n").length;
  const label = filename ?? lang ?? "text";

  const download = () => {
    const blob = new Blob([code], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename ?? `halotalk-${lang ?? "text"}.${LANG_EXT[lang ?? ""] ?? lang ?? "txt"}`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="codeblock">
      <div className="codeblock-head">
        <button
          data-testid="code-collapse"
          className="flex items-center gap-1.5 rounded px-1 py-0.5 transition-colors hover:bg-surface"
          aria-expanded={open}
          onClick={() => setUserOpen(!open)}
        >
          <IconChevronDown className={`h-3.5 w-3.5 transition-transform ${open ? "" : "-rotate-90"}`} />
          {label}
          {!open && <span className="text-[0.7rem] text-muted">· {lineCount} lines</span>}
        </button>
        <span className="ml-auto flex items-center gap-1">
          <button
            data-testid="code-download"
            className="rounded px-1.5 py-0.5 text-[0.7rem] hover:bg-surface transition-colors"
            title="Download as file"
            onClick={download}
          >
            <IconDownload className="h-3.5 w-3.5" />
          </button>
          <button
            className="rounded px-1.5 py-0.5 text-[0.7rem] hover:bg-surface transition-colors"
            onClick={() => {
              navigator.clipboard.writeText(code).then(() => {
                setCopied(true);
                setTimeout(() => setCopied(false), 1500);
              });
            }}
          >
            {copied ? "copied ✓" : "copy"}
          </button>
        </span>
      </div>
      {open ? (
        <pre>
          <code dangerouslySetInnerHTML={{ __html: html }} />
        </pre>
      ) : (
        <button
          className="block w-full truncate px-3 py-2 text-left text-xs text-muted hover:bg-surface/50"
          onClick={() => setUserOpen(true)}
        >
          {code.split("\n")[0]}…
        </button>
      )}
    </div>
  );
}

function MermaidBlock({ code }: { code: string }) {
  const [svg, setSvg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const theme = document.documentElement.dataset.theme ?? "light";

  useEffect(() => {
    let cancelled = false;
    import("mermaid")
      .then((m) => {
        const mermaid = m.default;
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: "strict",
          theme: theme === "dark" ? "dark" : "default",
        });
        return mermaid.render(`mmd-${Math.random().toString(36).slice(2)}`, code);
      })
      .then((r) => !cancelled && setSvg(r.svg))
      .catch((e: unknown) => !cancelled && setErr(String((e as Error)?.message ?? e)));
    return () => {
      cancelled = true;
    };
  }, [code, theme]);

  if (err)
    return (
      <>
        <div className="mermaid-error">mermaid: {err}</div>
        <CodeBlock code={code} lang="text" />
      </>
    );
  if (!svg) return <div className="mermaid-error">rendering diagram…</div>;
  return <div className="mermaid-box" dangerouslySetInnerHTML={{ __html: svg }} />;
}

export default function Markdown({ children, streaming }: { children: string; streaming?: boolean }) {
  const [plugins, setPlugins] = useState<{
    remark: unknown[];
    rehype: unknown[];
  }>(() => ({
    remark: remarkMathMod ? [remarkGfm, remarkMathMod] : [remarkGfm],
    rehype: rehypeKatexMod ? [rehypeKatexMod] : [],
  }));

  useEffect(() => {
    if (!maybeMath(children) || rehypeKatexMod) return;
    let cancelled = false;
    Promise.all([import("remark-math"), import("rehype-katex"), import("katex/dist/katex.min.css")])
      .then(([rm, rk]) => {
        if (cancelled) return;
        remarkMathMod = rm.default ?? rm;
        rehypeKatexMod = rk.default ?? rk;
        setPlugins({ remark: [remarkGfm, remarkMathMod], rehype: [rehypeKatexMod] });
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [children]);

  return (
    <div className="md">
      <ReactMarkdown
        remarkPlugins={plugins.remark as never}
        rehypePlugins={plugins.rehype as never}
        components={{
          // react-markdown 9 hands sanitized `node` snapshots with stripped
          // names/properties, so lang is read from the `code` component props.
          pre: ({ children }) => <>{children}</>,
          code: ({ className, node, children }) => {
            const { lang, filename } = parseFence(
              className,
              (node as unknown as { data?: { meta?: string } })?.data?.meta,
            );
            const raw = String(children ?? "");
            if (!className && !raw.includes("\n"))
              return <code className={className}>{children}</code>;
            const code = raw.replace(/\n$/, "");
            if (lang === "mermaid") return <MermaidBlock code={code} />;
            return <CodeBlock code={code} lang={lang} filename={filename} streaming={streaming} />;
          },
          a: ({ href, children: kids }) => (
            <a href={href} target="_blank" rel="noreferrer">
              {kids}
            </a>
          ),
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  );
}
