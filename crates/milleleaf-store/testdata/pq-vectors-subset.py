# Picks a small subset of NIST ACVP-Server and Wycheproof vectors for milleleaf-store's PQ signature tests.
import json, hashlib, sys
ACVP = "975de31eb83d87039ec88934fdc47d8c312b892d"
WP = "7a325932231bd7a724c791c8b0d65ea4d6d2f435"
def load(f):
    b = open(f, "rb").read(); return json.loads(b), hashlib.sha256(b).hexdigest()
out = {"sources": [], "mldsa65": {}, "slhdsa": {}}
def src(name, url, sha): out["sources"].append({"file": name, "url": url, "sha256": sha})
def pick(t, keys): return {k: t[k] for k in keys if k in t}
def groups(d, ps, extra=lambda g: True): return [g for g in d["testGroups"] if g["parameterSet"] == ps and extra(g)]
ext = lambda g: g.get("signatureInterface") == "external" and g.get("preHash") == "pure" and not g.get("externalMu")

for f in ["ML-DSA-keyGen-FIPS204", "ML-DSA-sigGen-FIPS204", "ML-DSA-sigVer-FIPS204",
          "SLH-DSA-keyGen-FIPS205", "SLH-DSA-sigGen-FIPS205", "SLH-DSA-sigVer-FIPS205"]:
    d, sha = load(f"acvp/{f}.json")
    src(f"{f}/internalProjection.json", f"https://github.com/usnistgov/ACVP-Server/blob/{ACVP}/gen-val/json-files/{f}/internalProjection.json", sha)
    if f.startswith("ML-DSA"):
        sets = [("ML-DSA-65", out["mldsa65"])]
    else:
        sets = [(p, out["slhdsa"].setdefault(p, {})) for p in ["SLH-DSA-SHA2-192s", "SLH-DSA-SHA2-128s"]]
    for ps, o in sets:
        if "keyGen" in f:
            g = groups(d, ps)[0]
            o["keyGen"] = [pick(t, ["tcId", "seed", "skSeed", "skPrf", "pkSeed", "pk", "sk"]) for t in g["tests"][:2]]
        elif "sigGen" in f:
            o["sigGen"] = []
            for g in groups(d, ps, ext):
                for t in g["tests"][:1]:
                    o["sigGen"].append({"tgId": g["tgId"], "deterministic": g["deterministic"],
                        **pick(t, ["tcId", "sk", "pk", "message", "context", "rnd", "additionalRandomness", "signature"])})
        else:
            o["sigVer"] = []
            for g in groups(d, ps, ext):
                seen = set()
                for t in g["tests"]:
                    r = t["reason"]
                    if r in seen or len(seen) >= 3: continue
                    seen.add(r)
                    o["sigVer"].append({"tgId": g["tgId"], **pick(t, ["tcId", "testPassed", "reason", "pk", "message", "context", "signature"])})

d, sha = load("wp/mldsa_65_verify_test.json")
src("testvectors_v1/mldsa_65_verify_test.json", f"https://github.com/C2SP/wycheproof/blob/{WP}/testvectors_v1/mldsa_65_verify_test.json", sha)
import re
wv, seen = [], set()
for g in d["testGroups"]:
    tests = []
    for t in g["tests"]:
        # one test per kind: "violate index 47 …" and "violate index 72 …" are one kind
        k = (t["result"], re.sub(r"\d+", "N", t["comment"]))
        if k in seen or len(tests) >= 3 or sum(1 for r, _ in seen if r == t["result"]) >= (8 if t["result"] == "valid" else 18): continue
        seen.add(k)
        tests.append({"tcId": t["tcId"], "msg": t["msg"], "ctx": t.get("ctx", ""), "sig": t["sig"],
                      "result": t["result"], "comment": t["comment"]})
    if tests: wv.append({"publicKey": g["publicKey"], "tests": tests})
out["mldsa65"]["wycheproofVerify"] = wv
d, sha = load("wp/mldsa_65_sign_seed_test.json")
src("testvectors_v1/mldsa_65_sign_seed_test.json", f"https://github.com/C2SP/wycheproof/blob/{WP}/testvectors_v1/mldsa_65_sign_seed_test.json", sha)
ws, seen = [], set()
for g in d["testGroups"]:
    for t in g["tests"]:
        k = (t["result"], t["comment"])
        if k in seen or len(ws) >= 4: continue
        seen.add(k)
        ws.append({"tcId": t["tcId"], "privateSeed": g["privateSeed"], "publicKey": g.get("publicKey", ""), "msg": t["msg"],
                   "ctx": t.get("ctx", ""), "sig": t["sig"], "result": t["result"], "comment": t["comment"]})
out["mldsa65"]["wycheproofSignSeed"] = ws
json.dump(out, open("pq-vectors.json", "w"), indent=0)
print(json.dumps({k: {kk: len(vv) for kk, vv in v.items()} if k == "mldsa65" else {p: {kk: len(vv) for kk, vv in s.items()} for p, s in v.items()} for k, v in out.items() if k != "sources"}))
print("wycheproof verify:", sum(len(g["tests"]) for g in wv), "tests in", len(wv), "groups")
