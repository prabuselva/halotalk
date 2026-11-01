// llama.cpp Prometheus metrics: prefill/decode speeds and spec-decode (MTP)
// accept rate, computed as counter deltas around one completion run.

export interface RunSpeeds {
  prefillTps: number | null;
  decodeTps: number | null;
  acceptPct: number | null;
}

export function parsePrometheus(text: string): Map<string, number> {
  const out = new Map<string, number>();
  for (const line of text.split("\n")) {
    if (!line || line.startsWith("#")) continue;
    const [name, value] = line.split(/\s+/, 2);
    if (!name.startsWith("llamacpp:")) continue;
    const v = Number.parseFloat(value ?? "");
    if (Number.isFinite(v)) out.set(name, v);
  }
  return out;
}

const NAMES = {
  promptTokens: ["llamacpp:prompt_tokens_total", "llamacpp:prompt_evaluations_total"],
  promptSecs: ["llamacpp:prompt_seconds_total", "llamacpp:prompt_evaluations_duration_seconds"],
  genTokens: ["llamacpp:tokens_predicted_total", "llamacpp:evaluations_total"],
  genSecs: ["llamacpp:tokens_predicted_seconds_total", "llamacpp:evaluations_duration_seconds"],
  draftTokens: ["llamacpp:spec_decode_num_draft_tokens_total"],
  accepted: ["llamacpp:spec_decode_num_accepted_tokens_total"],
};

const get = (m: Map<string, number>, names: string[]): number | undefined => {
  for (const n of names) if (m.has(n)) return m.get(n);
  return undefined;
};

const delta = (
  before: Map<string, number>,
  after: Map<string, number>,
  names: string[],
): number | null => {
  const b = get(before, names);
  const a = get(after, names);
  if (b === undefined || a === undefined) return null;
  return a - b;
};

const perSecond = (toks: number | null, secs: number | null): number | null =>
  toks !== null && secs !== null && toks > 0 && secs > 0 ? toks / secs : null;

export function computeSpeeds(
  before: Map<string, number> | null,
  after: Map<string, number> | null,
): RunSpeeds | null {
  if (!before || !after) return null;
  const draft = delta(before, after, NAMES.draftTokens);
  const accepted = delta(before, after, NAMES.accepted);
  return {
    prefillTps: perSecond(delta(before, after, NAMES.promptTokens), delta(before, after, NAMES.promptSecs)),
    decodeTps: perSecond(delta(before, after, NAMES.genTokens), delta(before, after, NAMES.genSecs)),
    acceptPct: draft !== null && accepted !== null && draft > 0 ? (accepted / draft) * 100 : null,
  };
}

const fmt = (n: number): string => n.toFixed(n >= 100 ? 0 : 1);

export function formatSpeeds(s: RunSpeeds | null): string {
  const pre = s?.prefillTps !== undefined && s?.prefillTps !== null ? `${fmt(s.prefillTps)} tok/s prefill` : "N/A prefill";
  const dec = s?.decodeTps !== undefined && s?.decodeTps !== null ? `${fmt(s.decodeTps)} tok/s decode` : "N/A decode";
  const acc = s?.acceptPct !== null && s?.acceptPct !== undefined ? ` · ${fmt(s.acceptPct)}% draft accept` : "";
  return `${pre} · ${dec}${acc}`;
}
