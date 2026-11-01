import { HaloLogo } from "./Icons";

type Props = {
  hasProviders: boolean;
  onOpenSettings: () => void;
};

export default function HaloChatEmpty({ hasProviders, onOpenSettings }: Props) {
  return (
    <div className="flex min-h-0 flex-1 items-center justify-center overflow-y-auto px-6">
      <div className="max-w-md text-center">
        <div className="mx-auto mb-5 flex h-14 w-14 items-center justify-center rounded-2xl bg-accent-soft">
          <HaloLogo className="h-8 w-8" />
        </div>
        <h2 className="text-lg font-semibold tracking-tight">Welcome to HaloTalk</h2>
        <p className="mt-2 text-sm leading-relaxed text-muted">
          Streaming chat with any OpenAI-compatible model — OpenAI, OpenRouter, Ollama, LM
          Studio. Attach images for vision models, tune temperature, tokens and reasoning
          per chat.
        </p>
        <div className="mt-5 flex items-center justify-center gap-2">
          <button
            className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white hover:bg-accent-strong"
            onClick={onOpenSettings}
          >
            {hasProviders ? "Model settings" : "Add a provider"}
          </button>
        </div>
        <p className="mt-6 text-xs text-muted">
          Type a message below to start a new conversation.
        </p>
      </div>
    </div>
  );
}
