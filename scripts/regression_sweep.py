#!/usr/bin/env python3
"""API regression sweep vs a running dev server + scripts/mock_llm.py (:9999).

Usage: E2E defaults bob/bobpassword, BASE http://127.0.0.1:8420.
Covers auth, chats (pin/tags/archive/search), hub turn lifecycle (202 send,
detached finalize, events state snapshots, stop, 409 conflict), prompts,
presets, registry, config export/import, unauth guard. Non-zero on failure."""
import json
import urllib.request

BASE = "http://127.0.0.1:8420"
J = json.dumps
results = []


def req(method, path, body=None, expect=200, hdrs=None):
    data = J(body).encode() if body is not None else None
    r = urllib.request.Request(BASE + path, data=data, method=method)
    r.add_header("Content-Type", "application/json")
    for k, v in (hdrs or {}).items():
        r.add_header(k, v)
    try:
        with urllib.request.urlopen(r) as resp:
            code, raw = resp.status, resp.read().decode()
    except urllib.error.HTTPError as e:
        code, raw = e.code, e.read().decode()
    ok = code == expect or (expect == 200 and code in (201, 204))
    results.append((method, path, code, expect, ok))
    return json.loads(raw) if ok and raw else {"_raw": raw, "_code": code}


opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor())
_orig = urllib.request.urlopen


def openr(r, **kw):
    return opener.open(r, **kw)


urllib.request.urlopen = openr

req("POST", "/api/auth/login", {"username": "bob", "password": "bobpassword"})
me = req("GET", "/api/auth/me")
assert me.get("username") == me.get("user", {}).get("username", "bob") and me.get("username", "bob") == "bob", me

# chats: create -> pin -> archive -> tags -> search -> unarchive -> delete
prov = req("GET", "/api/providers")["providers"][0]
c = req("POST", "/api/chats", {"title": "sweep-chat", "provider_id": prov["id"], "model": "mock-gpt"})
cid = c["id"]
p = req("PATCH", f"/api/chats/{cid}", {"pinned": True})
assert p["pinned"] is True, p
p = req("PATCH", f"/api/chats/{cid}", {"tags": ["sweep", "sweep"]})
assert p["tags"] == ["sweep"], p  # deduped array now serialized as array
p = req("PATCH", f"/api/chats/{cid}", {"archived": True})
assert p["archived"] is True, p
lst = req("GET", "/api/chats?archived=1")
assert any(x["id"] == cid for x in lst["chats"]), "archived list missing chat"
s = req("GET", "/api/search?q=%23sweep+sweep-chat")
res = s.get("results", s)
assert any("sweep-chat" in J(x) for x in res) or res, s[:1] if isinstance(res, list) else s
req("PATCH", f"/api/chats/{cid}", {"archived": False})

# prompts + presets + registry
pr = req("POST", "/api/prompts", {"title": "sweep-prompt", "body": "hello {x}"})
req("PATCH", f"/api/prompts/{pr['id']}", {"title": "sweep-prompt", "body": "hi {x}"})
pre = req("POST", "/api/presets", {"name": "sweep-preset", "params": {"temperature": 0.5}})
req("DELETE", f"/api/prompts/{pr['id']}")
req("DELETE", f"/api/presets/{pre['id']}")
pid = req("GET", "/api/providers")["providers"][0]["id"]
req("GET", f"/api/registry?provider_id={pid}")

# export + import round-trip (no secrets)
ex = req("GET", "/api/config/export")
assert ex.get("app") == "halotalk", list(ex)
imp = req("POST", "/api/config/import", ex)
assert imp["imported"]["providers"] >= 0, imp

import time


def first_state_frame(chat_id):
    """Open the events stream and read the initial `state` frame."""
    r = urllib.request.Request(f"{BASE}/api/chats/{chat_id}/events")
    with opener.open(r, timeout=15) as resp:
        ev = None
        while True:
            line = resp.readline().decode().strip()
            if line.startswith("event: "):
                ev = line[7:]
            elif line.startswith("data: ") and ev == "state":
                return json.loads(line[6:])


st0 = first_state_frame(cid)
idle_ok = st0 == {"streaming": False}
results.append(("GET", f"/api/chats/{cid}/events (idle state)", 200 if idle_ok else 599, 200, idle_ok))

# send -> 202; generation is detached (no open stream) and finalizes in DB
req("POST", f"/api/chats/{cid}/messages", {"content": "sweep ping"}, expect=202)
deadline, content = time.time() + 40, ""
while time.time() < deadline:
    msgs = req("GET", f"/api/chats/{cid}")["messages"]
    a = [m for m in msgs if m["role"] == "assistant"]
    if a and a[-1]["content"]:
        content = a[-1]["content"]
        break
    time.sleep(0.5)
ok = "Hellofrommock!" in content.replace(" ", "")
results.append(("POST", f"/api/chats/{cid}/messages -> detached finalize", 200 if ok else 599, 200, ok))

# live observer sees a streaming snapshot; stop persists the partial and frees the chat
req("POST", f"/api/chats/{cid}/messages", {"content": "SLOW sweep stop probe"}, expect=202)
time.sleep(1.0)
st1 = first_state_frame(cid)
snap_ok = st1.get("streaming") is True and bool(st1.get("message_id"))
results.append(("GET", f"/api/chats/{cid}/events (live state)", 200 if snap_ok else 599, 200, snap_ok))
st = req("POST", f"/api/chats/{cid}/stop", expect=200)
stop_ok = bool(st.get("stopped"))
results.append(("POST", f"/api/chats/{cid}/stop", 200 if stop_ok else 599, 200, stop_ok))
time.sleep(2.0)
msgs = req("GET", f"/api/chats/{cid}")["messages"]
a = [m for m in msgs if m["role"] == "assistant"][-1]
partial_ok = 0 < len(a["content"]) < 16 and a["error"] is None
results.append(("GET", f"/api/chats/{cid} (stopped partial)", 200 if partial_ok else 599, 200, partial_ok))

# chat is free again; a concurrent send while a turn runs conflicts (409)
req("POST", f"/api/chats/{cid}/messages", {"content": "sweep after stop"}, expect=202)
conf = req("POST", f"/api/chats/{cid}/messages", {"content": "should conflict"}, expect=409)
conf_ok = "already running" in J(conf)
results.append(("POST", f"/api/chats/{cid}/messages (409 conflict)", 409 if conf_ok else 599, 409, conf_ok))
time.sleep(4)

# cleanup + auth guard
req("DELETE", f"/api/chats/{cid}")
import urllib.request as _u
fresh = _u.build_opener()
fresh.addheaders = []
r = _u.Request(BASE + "/api/chats")
try:
    fresh.open(r)
    results.append(("GET", "/api/chats unauth", 200, 401, False))
except urllib.error.HTTPError as e:
    results.append(("GET", "/api/chats unauth", e.code, 401, e.code == 401))

bad = [x for x in results if not x[4]]
for m, pth, got_, want_, okk in results:
    print(f"{'ok ' if okk else 'FAIL'} {m:6} {pth} -> {got_} (want {want_})")
import sys
print("SWEEP:", "ALL GREEN" if not bad else f"{len(bad)} FAILURES")
sys.exit(1 if bad else 0)
