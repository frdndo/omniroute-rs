#!/usr/bin/env python3
"""Extract provider catalog from OmniRoute TS registry → providers.json (v2).

Handles:
- inline `models: [...]` arrays (v1)
- `models: CHAT_OPENAI_COMPAT_MODELS.<key>` references
- `models: buildModels([...])` / `models: [...spread]` patterns
- `models: SOME_CONST` (exported arrays from shared.ts)
"""
import json, os, re, glob

BASE = os.environ.get("OMNIROUTE_SRC", "/root/OmniRoute") + "/open-sse/config/providers"
REG = f"{BASE}/registry"
OUT = os.environ.get(
    "PROVIDERS_OUT",
    os.path.join(os.path.dirname(__file__), "..", "rust-core", "omniroute-providers", "data", "providers.json"),
)

# ── Parse shared.ts exports ────────────────────────────────────────────
shared_src = open(f"{BASE}/shared.ts", encoding="utf-8", errors="ignore").read()

# All top-level exported consts (name → value text)
consts = {}
for m in re.finditer(r"export const (\w+)[^=]*=\s*([{[])", shared_src):
    name = m.group(1)
    start = m.start(2)
    opener = m.group(2)
    closer = "}" if opener == "{" else "]"
    depth = 0
    i = start
    in_str = False
    while i < len(shared_src):
        c = shared_src[i]
        if c == '"' and (i == 0 or shared_src[i-1] != "\\"):
            in_str = not in_str
        if not in_str:
            if c == opener:
                depth += 1
            elif c == closer:
                depth -= 1
                if depth == 0:
                    break
        i += 1
    consts[name] = shared_src[start:i+1]

def buildmodels_ids(text):
    """Extract string ids from buildModels([...]) or bare arrays of strings."""
    ids = []
    # buildModels([ "a", "b" ])
    for mm in re.finditer(r'buildModels\(\s*\[(.*?)\]', text, re.S):
        ids += re.findall(r'"([^"]+)"', mm.group(1))
    # bare array of strings: [ "a", { id: "b" } ]
    arr = re.findall(r'\[\s*((?:"[^"]*"\s*,?\s*)+)\]', text)
    for a in arr:
        ids += re.findall(r'"([^"]+)"', a)
    # object entries { id: "x", ... }
    for mm in re.finditer(r'\{\s*id:\s*"([^"]+)"', text):
        ids.append(mm.group(1))
    return ids

# Pre-resolve named model lists in shared.ts (CHAT_OPENAI_COMPAT_MODELS etc.)
shared_model_lists = {}  # constName.providerKey -> [ids]  AND  constName -> [ids]
for name, text in consts.items():
    if "Record<string, RegistryModel[]>" in shared_src[max(0, shared_src.find(name)-200):shared_src.find(name)+len(name)]:
        pass
    # provider-keyed map: { deepinfra: buildModels([...]), ... }
    for mm in re.finditer(r'(\w+):\s*(buildModels\(\[.*?\]\)|\[.*?\])', text, re.S):
        key, val = mm.group(1), mm.group(2)
        shared_model_lists[f"{name}.{key}"] = buildmodels_ids(val)
    # plain array
    ids = buildmodels_ids(text)
    if ids:
        shared_model_lists[name] = ids

# Also scan open-sse/config/*.ts for named model consts (glmProvider.ts, etc.)
for cfg in glob.glob(os.path.join(BASE, "..", "*.ts")):
    csrc = open(cfg, encoding="utf-8", errors="ignore").read()
    for cm in re.finditer(r"(?:export )?const (\w+)[^=]*=\s*(?:Object\.freeze\(\s*)?([\[{])", csrc):
        cname = cm.group(1)
        opener = cm.group(2)
        closer = "}" if opener == "{" else "]"
        cstart = cm.start(2)
        depth = 0
        k = cstart
        in_str = False
        while k < len(csrc):
            c = csrc[k]
            if c == '"' and (k == 0 or csrc[k-1] != "\\"):
                in_str = not in_str
            if not in_str:
                if c == opener:
                    depth += 1
                elif c == closer:
                    depth -= 1
                    if depth == 0:
                        break
            k += 1
        ids = buildmodels_ids(csrc[cstart:k+1])
        if ids:
            shared_model_lists.setdefault(cname, [])
            for mid in ids:
                if mid not in shared_model_lists[cname]:
                    shared_model_lists[cname].append(mid)

# ── Parse each registry entry ──────────────────────────────────────────
def extract_provider(src):
    """Return (provider_dict, models_ids) or None."""
    for m in re.finditer(r"export const \w+Provider: RegistryEntry = \{", src):
        start = m.end() - 1
        depth = 0
        i = start
        in_str = False
        while i < len(src):
            c = src[i]
            if c == '"' and (i == 0 or src[i-1] != "\\"):
                in_str = not in_str
            if not in_str:
                if c == "{":
                    depth += 1
                elif c == "}":
                    depth -= 1
                    if depth == 0:
                        break
            i += 1
        obj = src[start:i+1]

        def field(name):
            mm = re.search(rf'^\s*{name}:\s*"([^"]+)"', obj, re.M)
            return mm.group(1) if mm else None

        pid = field("id") or field("alias")
        if not pid:
            continue
        fmt = field("format")
        base_url = field("baseUrl")
        auth_type = field("authType")
        auth_header = field("authHeader")

        # ── models resolution ──
        models = []
        mm = re.search(r"models:\s*", obj)
        if mm:
            mstart = mm.end()
            rest = obj[mstart:]
            models_block = None

            if rest.startswith("[") or rest.startswith("..."):
                # bracket array or spread-of-const
                depth = 0
                j = 0
                in_str = False
                while j < len(rest):
                    c = rest[j]
                    if c == '"' and (j == 0 or rest[j-1] != "\\"):
                        in_str = not in_str
                    if not in_str:
                        if c == "[":
                            depth += 1
                        elif c == "]":
                            depth -= 1
                            if depth == 0:
                                break
                    j += 1
                models_block = rest[:j+1]
            else:
                # direct identifier reference: models: SOME_CONST or models: MAP.KEY
                idm = re.match(r"([A-Za-z_]\w*(?:\.\w+)*)\s*,?\s*$", rest.split("\n")[0])
                if idm:
                    ident = idm.group(1)
                    if "." in ident:
                        # CHAT_OPENAI_COMPAT_MODELS.deepinfra style — resolve from shared map
                        for mid in shared_model_lists.get(ident, []):
                            if not any(m["id"] == mid for m in models):
                                models.append({"id": mid, "name": mid})
                    else:
                        # plain const — search same-file, siblings, then shared.ts
                        candidates = [src] + dir_srcs + [shared_src]
                        for src2 in candidates:
                            cm = re.search(rf"(?:const|export const) {ident}[^=]*=\s*([\[{{])", src2)
                            if cm:
                                opener = cm.group(1)
                                closer = "}" if opener == "{" else "]"
                                cstart = cm.start(1)
                                depth = 0
                                k = cstart
                                in_str = False
                                while k < len(src2):
                                    c = src2[k]
                                    if c == '"' and (k == 0 or src2[k-1] != "\\"):
                                        in_str = not in_str
                                    if not in_str:
                                        if c == opener:
                                            depth += 1
                                        elif c == closer:
                                            depth -= 1
                                            if depth == 0:
                                                break
                                    k += 1
                                models_block = src2[cstart:k+1]
                                break

            if models_block:
                # inline ids
                for mm2 in re.finditer(r'id:\s*"([^"]+)"', models_block):
                    mid = mm2.group(1)
                    seg = models_block[mm2.start():mm2.start()+200]
                    nm = re.search(r'name:\s*"([^"]+)"', seg)
                    models.append({"id": mid, "name": nm.group(1) if nm else mid})

                # bare strings (buildModels style)
                for mm2 in re.finditer(r'"([^"]+)"', models_block):
                    if not any(m["id"] == mm2.group(1) for m in models):
                        models.append({"id": mm2.group(1), "name": mm2.group(1)})

                # referenced consts: CHAT_OPENAI_COMPAT_MODELS.deepinfra / ...spread
                for ref in re.finditer(r"(?:CHAT_OPENAI_COMPAT_MODELS\.(\w+)|\.\.\.(\w+))", models_block):
                    key = f"CHAT_OPENAI_COMPAT_MODELS.{ref.group(1)}" if ref.group(1) else ref.group(2)
                    for mid in shared_model_lists.get(key, []):
                        if not any(m["id"] == mid for m in models):
                            models.append({"id": mid, "name": mid})

        return {
            "id": pid,
            "format": fmt,
            "baseUrl": base_url,
            "authType": auth_type,
            "authHeader": auth_header,
            "modelCount": len(models),
            "models": models,
        }

providers = []
skipped = []
for f in sorted(glob.glob(f"{REG}/**/index.ts", recursive=True)):
    src = open(f, encoding="utf-8", errors="ignore").read()
    # sibling files in the same dir (const definitions live there sometimes)
    dir_srcs = []
    d = os.path.dirname(f)
    for sf in glob.glob(f"{d}/*.ts"):
        if os.path.abspath(sf) != os.path.abspath(f):
            dir_srcs.append(open(sf, encoding="utf-8", errors="ignore").read())

    # register sibling consts into the resolver (spread targets)
    for s in dir_srcs:
        for cm in re.finditer(r"(?:export )?const (\w+)[^=]*=\s*([\[{])", s):
            cname = cm.group(1)
            opener = cm.group(2)
            closer = "}" if opener == "{" else "]"
            cstart = cm.start(2)
            depth = 0
            k = cstart
            in_str = False
            while k < len(s):
                c = s[k]
                if c == '"' and (k == 0 or s[k-1] != "\\"):
                    in_str = not in_str
                if not in_str:
                    if c == opener:
                        depth += 1
                    elif c == closer:
                        depth -= 1
                        if depth == 0:
                            break
                k += 1
            shared_model_lists.setdefault(cname, [])
            for mid in buildmodels_ids(s[cstart:k+1]):
                if mid not in shared_model_lists[cname]:
                    shared_model_lists[cname].append(mid)
    p = extract_provider(src)
    if p and p["models"]:
        providers.append(p)
    elif p:
        skipped.append(p["id"])

# Dedupe by id (later files win)
seen = {}
for p in providers:
    seen[p["id"]] = p
providers = sorted(seen.values(), key=lambda p: p["id"])

total_models = sum(p["modelCount"] for p in providers)
print(f"Providers: {len(providers)} | Models: {total_models} | Skipped: {len(skipped)}")
print("Skipped:", ", ".join(skipped[:20]))

os.makedirs(os.path.dirname(OUT), exist_ok=True)
with open(OUT, "w") as fh:
    fh.write(json.dumps(providers, indent=1))
print(f"Written: {OUT} ({os.path.getsize(OUT)} bytes)")
