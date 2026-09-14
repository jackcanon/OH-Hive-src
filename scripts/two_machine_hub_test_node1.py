#!/usr/bin/env python3
"""
ADR-025 physical two-machine smoke test -- node1 (the hub owner), runs on the SAME machine as
`local_hub serve` (Midgaard). Sif's `scripts/test_local_hub_smoke.py` proved this over loopback
with two processes on one machine; this is the same RPC sequence split across two real machines
on the LAN, closing the one gap she flagged (task #199, 2026-09-13).

Usage: python3 two_machine_hub_test_node1.py <hub-origin> <owner-creds.json> <card-id>
  e.g. python3 two_machine_hub_test_node1.py http://192.168.1.143:8787 owner.json 3f9b...

Claims the card as the owner, checkpoints it, then releases it -- so the second machine (node2)
can claim the same card and find the checkpoint waiting for it over the real network.
"""
import sys
import json
import uuid
import urllib.request


def call(origin, path, body, key=None):
    headers = {"content-type": "application/json"}
    if key:
        headers["Authorization"] = "Bearer " + key
    req = urllib.request.Request(origin + path, data=json.dumps(body).encode(), headers=headers)
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.load(r)


def rpc(origin, key, session, method, params=None):
    return call(origin, "/local/v1/rpc", {"session": session, "method": method, "params": params or {}}, key)


def main():
    origin, creds_path, card_id = sys.argv[1], sys.argv[2], sys.argv[3]
    owner = json.load(open(creds_path))
    session = str(uuid.uuid4())
    caps = {
        "hardware": {"cpu_model": "midgaard", "cpu_cores": 1, "ram_bytes": 1, "gpu_vendor": "none", "disk_free_bytes": 1},
        "modalities": ["text"],
        "models": [],
        "allow_internet": False,
        "tools_level": "inference_only",
    }
    rpc(origin, owner["raw_key"], session, "check_in", {"caps": caps, "region": None})
    claim = rpc(origin, owner["raw_key"], session, "claim_card")
    print("Midgaard (owner) claim_card:", claim)
    assert claim.get("status") == "leased", f"expected to claim the card, got: {claim}"
    usage = {"tokens_in": 1, "tokens_out": 1, "compute_seconds": 0}
    rpc(origin, owner["raw_key"], session, "checkpoint", {
        "card_id": card_id, "step": 1, "state": {"from": "midgaard over real LAN"}, "usage": usage,
    })
    rpc(origin, owner["raw_key"], session, "release_card", {
        "card_id": card_id, "reason": "handing off to the second machine",
    })
    print("PASS (node1/Midgaard): claimed, checkpointed, released. Now run node2's script on the other machine.")


if __name__ == "__main__":
    main()
