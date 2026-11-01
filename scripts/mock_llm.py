#!/usr/bin/env python3
"""Mock LLM provider for HaloTalk e2e (port 9999).

Families served under one server:
  OpenAI-compat   /v1/models  /v1/chat/completions  /v1/embeddings
                  /v1/audio/speech  /v1/audio/transcriptions
  Anthropic       /anthropic/v1/messages            (SSE + non-stream)
  Gemini          /gemini/v1beta/models[:generateContent|
                  :streamGenerateContent?alt=sse]   (+ /gemini/v1beta/models)
  Web search      /search   (POST {"query"} — HALO_WEB_SEARCH_URL target)

Behaviours: default greeting per family; image parts → "(saw an image)";
web_search tool round when tools are advertised (then echo "(used web search: …)");
title prompts ("…title…") → "Mock Title: <first words>" (non-stream);
"SLOW" → chunked with delays. Requests dumped to /tmp/opencode/last_req*.json.
Deterministic FNV-1a bag-of-words embeddings (256-dim unit vectors).
"""
import base64
import json
import os
import re
import struct
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

DUMP_DIR = "/tmp/opencode"
PORT = 9999
HOST = os.environ.get("HALO_MOCK_HOST", "127.0.0.1")


def dump(name, obj):
    try:
        with open(f"{DUMP_DIR}/{name}", "w") as f:
            json.dump(obj, f, indent=2)
    except OSError:
        pass


def embed(text):
    """Deterministic unit bag-of-words vector (256 dims) — FNV-1a per word."""
    v = [0.0] * 256
    for w in re.findall(r"[a-z0-9]+", str(text).lower()):
        h = 2166136261
        for ch in w:
            h = ((h ^ ord(ch)) * 16777619) & 0xFFFFFFFF
        v[h % 256] += 1.0
    norm = sum(x * x for x in v) ** 0.5 or 1.0
    return [x / norm for x in v]


# ------------------------------------------------------------------ helpers


def text_of(content):
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return " ".join(
            p.get("text", "")
            for p in content
            if isinstance(p, dict) and p.get("type") in ("text", None) and p.get("text")
        )
    return ""


def has_image_parts(content):
    if not isinstance(content, list):
        return False
    return any(
        isinstance(p, dict) and (p.get("type") == "image_url" or "inline_data" in p or "image" in p)
        for p in content
    )


def last_user_text(messages):
    for m in reversed(messages):
        if m.get("role") == "user":
            return text_of(m.get("content"))
    return ""


def title_reply(messages):
    words = last_user_text(messages).split()[:3]
    return "Mock Title: " + " ".join(words)


def is_title_request(body_str):
    return "title" in body_str.lower()


def openai_reply_body(req, greeting="Hello from mock!"):
    """Decide final assistant text for a chat request (no tool round)."""
    messages = req.get("messages", [])
    for m in messages:
        if m.get("role") == "tool" or m.get("tool_call_id"):
            content = text_of(m.get("content"))
            return f"(used web search: {content[:120]})"
    lu = last_user_text(messages)
    if any(has_image_parts(m.get("content")) for m in messages if m.get("role") == "user"):
        return f"{greeting} (saw an image)"
    if "JSONBLOCK" in lu.upper():
        rows = ",\n".join(
            f'    {{"id": {i}, "name": "item-{i}", "active": {str(i % 3 != 0).lower()}}}'
            for i in range(1, 21)
        )
        return (
            "Here is the config:\n\n```json\n{\n  \"version\": 2,\n  \"items\": [\n"
            + rows
            + "\n  ]\n}\n```\n\nDone."
        )
    if lu:
        return greeting
    return greeting


def wants_tool_call(req):
    if not req.get("tools"):
        return False
    for m in req.get("messages", []):
        if m.get("role") in ("tool",) or m.get("tool_call_id") or m.get("role") == "function":
            return False
    return True


def tool_query(req):
    q = last_user_text(req)
    q = re.sub(r"\b(search|searching|web|please|look up)\b", "", q, flags=re.I)
    q = re.sub(r"\s+", " ", q).strip()
    return q or "mock query"


def sse_event(handler, data, event=None):
    if event:
        handler.wfile.write(f"event: {event}\n".encode())
    handler.wfile.write(f"data: {data}\n\n".encode())
    handler.wfile.flush()


def sse_chunk(handler, delta, finish=None):
    body = {"choices": [{"delta": delta, "index": 0}]}
    if finish:
        body["choices"][0]["finish_reason"] = finish
    sse_event(handler, json.dumps(body))


def stream_openai(handler, req):
    handler.send_response(200)
    handler.send_header("Content-Type", "text/event-stream")
    handler.end_headers()
    slow = "SLOW" in json.dumps(req).upper()
    try:
        if wants_tool_call(req):
            q = tool_query(req)
            args = json.dumps({"query": q})
            sse_chunk(handler, {"tool_calls": [{"index": 0, "id": "tool_mock1", "type": "function", "function": {"name": "web_search", "arguments": ""}}]})
            mid = len(args) // 2
            sse_chunk(handler, {"tool_calls": [{"index": 0, "function": {"arguments": args[:mid]}}]})
            sse_chunk(handler, {"tool_calls": [{"index": 0, "function": {"arguments": args[mid:]}}]})
            sse_chunk(handler, {}, finish="tool_calls")
        else:
            sse_chunk(handler, {"role": "assistant", "reasoning_content": "thinking…"})
            reply = openai_reply_body(req)
            step = max(1, len(reply) // 6)
            for i in range(0, len(reply), step):
                sse_chunk(handler, {"content": reply[i:i + step]})
                if slow:
                    time.sleep(1.2)
            sse_chunk(handler, {}, finish="stop")
        handler.wfile.write(b"data: [DONE]\n\n")
        handler.wfile.flush()
    except BrokenPipeError:
        pass


# ------------------------------------------------------------------ anthropic


def anthropic_stream(handler, req):
    handler.send_response(200)
    handler.send_header("Content-Type", "text/event-stream")
    handler.end_headers()
    reply = {"kind": "text"}
    tools = req.get("tools")
    msgs = req.get("messages", [])
    has_tool_result = any(
        isinstance(c, dict) and c.get("type") == "tool_result"
        for m in msgs
        if m.get("role") == "user" and isinstance(m.get("content"), list)
        for c in m["content"]
    )
    try:
        ev = lambda name, obj: sse_event(handler, json.dumps(obj), event=name)  # noqa: E731
        ev("message_start", {"type": "message_start", "message": {"id": "msg_mock", "role": "assistant", "content": []}})
        if tools and not has_tool_result:
            q = tool_query({"messages": [{"role": "user", "content": anthropic_last_user(req)}]})
            args = json.dumps({"query": q})
            ev("content_block_start", {"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "tool_mock1", "name": "web_search", "input": {}}})
            mid = len(args) // 2
            ev("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": args[:mid]}})
            ev("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": args[mid:]}})
            ev("content_block_stop", {"type": "content_block_stop", "index": 0})
            ev("message_delta", {"type": "message_delta", "delta": {"stop_reason": "tool_use"}})
        else:
            ev("content_block_start", {"type": "content_block_start", "index": 0, "content_block": {"type": "thinking"}})
            ev("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "thinking…"}})
            ev("content_block_stop", {"type": "content_block_stop", "index": 0})
            ev("content_block_start", {"type": "content_block_start", "index": 1, "content_block": {"type": "text"}})
            text = anthropic_reply(req)
            step = max(1, len(text) // 6)
            for i in range(0, len(text), step):
                ev("content_block_delta", {"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": text[i:i + step]}})
            ev("content_block_stop", {"type": "content_block_stop", "index": 1})
            ev("message_delta", {"type": "message_delta", "delta": {"stop_reason": "end_turn"}})
        ev("message_stop", {"type": "message_stop"})
    except BrokenPipeError:
        pass


def anthropic_last_user(req):
    for m in reversed(req.get("messages", [])):
        if m.get("role") == "user":
            c = m.get("content")
            if isinstance(c, str):
                return c
            if isinstance(c, list):
                return " ".join(b.get("text", "") for b in c if isinstance(b, dict) and b.get("type") == "text")
    return ""


def anthropic_reply(req):
    msgs = req.get("messages", [])
    for m in msgs:
        if m.get("role") == "user" and isinstance(m.get("content"), list):
            if any(isinstance(b, dict) and b.get("type") in ("tool_result",) for b in m["content"]):
                for b in m["content"]:
                    if isinstance(b, dict) and b.get("type") == "tool_result":
                        inner = b.get("content")
                        s = inner if isinstance(inner, str) else json.dumps(inner)
                        return f"(used web search: {s[:120]})"
            if any(isinstance(b, dict) and b.get("type") == "image" for b in m["content"]):
                return "Hello from mock-anthropic! (saw an image)"
    return "Hello from mock-anthropic!"


def anthropic_nonstream(req):
    if is_title_request(json.dumps(req)) and not req.get("tools"):
        text = title_reply([{"role": "user", "content": anthropic_last_user(req)}])
    else:
        text = anthropic_reply(req)
    return {
        "id": "msg_mock",
        "role": "assistant",
        "content": [{"type": "text", "text": text}],
        "stop_reason": "end_turn",
    }


# --------------------------------------------------------------------- gemini


def gemini_last_user(req):
    for m in reversed(req.get("contents", [])):
        if m.get("role") == "user":
            parts = m.get("parts", [])
            if any("functionResponse" in p for p in parts):
                return None  # tool round continuation
            return " ".join(p.get("text", "") for p in parts if "text" in p)
    return ""


def gemini_tool_response(req):
    for m in req.get("contents", []):
        for p in m.get("parts", []):
            if "functionResponse" in p:
                inner = p["functionResponse"].get("response")
                return json.dumps(inner)[:120] if not isinstance(inner, str) else inner[:120]
    return ""


def gemini_reply(req):
    tools = req.get("tools")
    if tools and gemini_tool_response(req) is not None:
        return f"(used web search: {gemini_tool_response(req)})"
    parts_all = [p for m in req.get("contents", []) for p in m.get("parts", [])]
    if any("inline_data" in p or "inlineData" in p for p in parts_all):
        return "Hello from mock-gemini! (saw an image)"
    return "Hello from mock-gemini!"


def gemini_nonstream(req):
    body_str = json.dumps(req)
    if is_title_request(body_str) and not req.get("tools"):
        text = title_reply([{"role": "user", "content": gemini_last_user(req) or ""}])
        return {"candidates": [{"content": {"parts": [{"text": text}]}, "finishReason": "STOP"}]}
    if req.get("tools") and gemini_tool_response(req) == "":
        q = {"query": (gemini_last_user(req) or "mock query")}
        q["query"] = re.sub(r"\b(search|searching|web|please|look up)\b", "", q["query"], flags=re.I).strip() or "mock query"
        return {
            "candidates": [{
                "content": {"parts": [{"functionCall": {"name": "web_search", "args": q}}]},
                "finishReason": "STOP",
            }]
        }
    return {"candidates": [{"content": {"parts": [{"text": gemini_reply(req)}]}, "finishReason": "STOP"}]}


def gemini_stream(req):
    return gemini_nonstream(req)


# ---------------------------------------------------------------- WAV for TTS


def silent_wav(seconds=0.4, rate=8000):
    n = int(seconds * rate)
    data = b"\x00\x00" * n
    hdr = struct.pack(
        "<4sI4s4sIHHIIHH4sI",
        b"RIFF", 36 + len(data), b"WAVE", b"fmt ", 16, 1, 1, rate, rate * 2, 2, 16,
        b"data", len(data),
    )
    return hdr + data


def parse_multipart(content_type, body):
    m = re.search(r'boundary="?([^";]+)"?', content_type)
    if not m:
        return {}
    boundary = m.group(1).encode()
    out = {}
    for part in body.split(b"--" + boundary):
        part = part.strip(b"\r\n")
        if not part or part == b"--":
            continue
        head, _, payload = part.partition(b"\r\n\r\n")
        heads = head.decode(errors="replace")
        name_m = re.search(r'name="([^"]*)"', heads)
        if not name_m:
            continue
        fname_m = re.search(r'filename="([^"]*)"', heads)
        ct_m = re.search(r"Content-Type:\s*([^\r\n]+)", heads)
        out[name_m.group(1)] = {
            "filename": fname_m.group(1) if fname_m else None,
            "content_type": ct_m.group(1).strip() if ct_m else "application/octet-stream",
            "data": payload,
        }
    return out


# ------------------------------------------------------------------ handler


class H(BaseHTTPRequestHandler):
    def log_message(self, format, *args):  # noqa: A002
        pass

    def _cors(self):
        self.send_header("Access-Control-Allow-Origin", "*")

    def do_GET(self):  # noqa: N802
        p = self.path.split("?")[0]
        if p == "/v1/models":
            self.json({"data": [{"id": "mock-gpt", "object": "model"}, {"id": "mock-vision", "object": "model"}]})
        elif p == "/anthropic/v1/models":
            self.json({"data": [{"id": "mock-claude", "type": "model"}]})
        elif p == "/gemini/v1beta/models":
            self.json({"models": [{"name": "models/mock-gemini", "displayName": "Mock Gemini"}]})
        else:
            self.json({"error": "not found"}, 404)

    def json(self, obj, code=200):
        raw = json.dumps(obj).encode()
        self.send_response(code)
        self._cors()
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_POST(self):  # noqa: N802
        p = self.path.split("?")[0]
        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length)
        try:
            if p == "/v1/audio/transcriptions":
                return self.transcribe(body)
            req = json.loads(body)
        except (ValueError, json.JSONDecodeError):
            return self.json({"error": "invalid JSON"}, 400)
        if p == "/v1/chat/completions":
            dump("last_req.json", req)
            if req.get("stream"):
                return stream_openai(self, req)
            if is_title_request(json.dumps(req)) and not req.get("tools"):
                content = title_reply(req.get("messages", []))
            else:
                content = openai_reply_body(req)
            return self.json({"choices": [{"message": {"role": "assistant", "content": content}}]})
        if p == "/v1/embeddings":
            dump("last_embed.json", req)
            inputs = req.get("input", [])
            if isinstance(inputs, str):
                inputs = [inputs]
            return self.json({"data": [{"index": i, "embedding": embed(t)} for i, t in enumerate(inputs)]})
        if p == "/v1/audio/speech":
            dump("last_req_speech.json", req)
            wav = silent_wav()
            self.send_response(200)
            self._cors()
            self.send_header("Content-Type", "audio/wav")
            self.send_header("Content-Length", str(len(wav)))
            self.end_headers()
            return self.wfile.write(wav)
        if p == "/anthropic/v1/messages":
            dump("last_req_anth.json", req)
            if req.get("stream"):
                return anthropic_stream(self, req)
            return self.json(anthropic_nonstream(req))
        if p.endswith(":generateContent"):
            dump("last_req_gem.json", req)
            return self.json(gemini_nonstream(req))
        if p.endswith(":streamGenerateContent"):
            dump("last_req_gem.json", req)
            self.send_response(200)
            self._cors()
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            try:
                if req.get("tools") and gemini_tool_response(req) == "":
                    resp = gemini_nonstream(req)
                    sse_event(self, json.dumps(resp))
                else:
                    reply = gemini_reply(req)
                    step = max(1, len(reply) // 6)
                    sse_event(self, json.dumps({"candidates": [{"content": {"parts": [{"text": "thinking…", "thought": True}]}}]}))
                    for i in range(0, len(reply), step):
                        sse_event(self, json.dumps({"candidates": [{"content": {"parts": [{"text": reply[i:i + step]}]}}]}))
                    sse_event(self, json.dumps({"candidates": [{"content": {"parts": []}, "finishReason": "STOP"}]}))
            except BrokenPipeError:
                pass
            return None
        if p == "/search":
            q = req.get("query", "")
            return self.json({"query": q, "results": [
                {"title": f"Mock result for {q}", "url": "https://example.com/mock",
                 "content": f"Mock search result about: {q}"},
            ]})
        return self.json({"error": "not found"}, 404)

    def transcribe(self, body):
        fields = parse_multipart(self.headers.get("Content-Type", ""), body)
        f = fields.get("file", {})
        n = len(f.get("data", b""))
        name = f.get("filename") or "audio"
        dump("last_audio_transcribe.json", {"filename": name, "bytes": n, "content_type": f.get("content_type")})
        self.json({"text": f"mock transcript of {name} ({n} bytes)"})


if __name__ == "__main__":
    import os
    os.makedirs(DUMP_DIR, exist_ok=True)
    print(f"mock llm on :{PORT} (openai+anthropic+gemini+audio+search+embed)")
    ThreadingHTTPServer((HOST, PORT), H).serve_forever()
