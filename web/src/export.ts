import type { Chat, Message } from "./types";

function download(filename: string, mime: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type: mime }));
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

function slug(title: string): string {
  return (
    title
      .toLowerCase()
      .replace(/[^\w]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 60) || "chat"
  );
}

export function exportChatMarkdown(chat: Chat, messages: Message[]) {
  const lines: string[] = [`# ${chat.title}`, ""];
  for (const m of messages.filter((m) => m.active !== false)) {
    if (m.role === "system") continue;
    lines.push(`**${m.role === "user" ? "User" : "Assistant"}:**`, "");
    lines.push(m.content, "");
    if (m.images?.length) {
      for (const img of m.images) lines.push(`![image](${img})`);
      lines.push("");
    }
  }
  download(`${slug(chat.title)}.md`, "text/markdown", lines.join("\n"));
}

export function exportChatJson(chat: Chat, messages: Message[]) {
  download(
    `${slug(chat.title)}.json`,
    "application/json",
    JSON.stringify(
      {
        title: chat.title,
        model: chat.model,
        system_prompt: chat.system_prompt,
        exported_at: new Date().toISOString(),
        messages: messages
          .filter((m) => m.active !== false)
          .map(({ id, role, content, images, created_at }) => ({
          id,
          role,
          content,
          ...(images?.length ? { images } : {}),
          created_at,
        })),
      },
      null,
      2,
    ),
  );
}
