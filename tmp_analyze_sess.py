# AI生成
import json, collections

p = r"C:\Users\sjxx\Downloads\sess_dd7ee53954934584bf4e9732719bba5e-1792121889706.jsonl"
lines = [json.loads(l) for l in open(p, encoding="utf-8") if l.strip()]

turns = {}
for d in lines:
    if d.get("record_type") == "turn":
        t = d["turn"]
        turns.setdefault(t["turn_id"], {"summary": t.get("summary"), "order": t.get("user_turn_index"), "items": []})
    elif d.get("record_type") == "item":
        it = d["item"]
        turns.setdefault(it.get("turn_id"), {"summary": "?", "order": 0, "items": []})
        turns[it.get("turn_id")]["items"].append(it)

def has_img(x):
    s = json.dumps(x, ensure_ascii=False)
    return ("data:image" in s) or ('"media_kind"' in s) or ("image_url" in s)

for tid, tv in sorted(turns.items(), key=lambda kv: kv[1]["order"]):
    print("=" * 90)
    print(f"TURN {tv['order']}: {tv['summary']}")
    for it in sorted(tv["items"], key=lambda x: x.get("item_index", 0)):
        k = it.get("kind"); pl = it.get("payload", {})
        if k == "assistant_message":
            ms = (pl.get("meta") or {}).get("message_stats") or {}
            print(f"  idx{it.get('item_index'):>3} assistant r{it.get('model_round')} "
                  f"ctx={ms.get('contextTokens')} occ={ms.get('context_occupancy_tokens')} "
                  f"content={len(pl.get('content') or '')} reasoning={len(pl.get('reasoning_content') or '')} "
                  f"tc={len(pl.get('tool_calls') or [])} img={has_img(pl)}")
        elif k == "tool_call":
            print(f"  idx{it.get('item_index'):>3} tool_call r{it.get('model_round')} name={pl.get('name') or pl.get('tool_name')} img={has_img(pl)}")
        elif k == "tool_message":
            c = pl.get("content")
            print(f"  idx{it.get('item_index'):>3} tool_msg r{it.get('model_round')} len={len(json.dumps(c, ensure_ascii=False))} img={has_img(pl)}")
        elif k in ("user_message", "queue"):
            print(f"  idx{it.get('item_index'):>3} {k} {json.dumps(pl.get('content'), ensure_ascii=False)[:60]}")