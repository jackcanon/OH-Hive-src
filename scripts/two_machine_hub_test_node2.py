#!/usr/bin/env python3
"""
ADR-025 physical two-machine smoke test -- node2, runs on the SECOND machine (e.g. Overgaard),
reached over the real LAN. No Rust toolchain or repo checkout needed here -- just Python 3 and
network access to the hub. Pairs fresh with the code node1's machine printed, claims the card
node1 checkpointed and released, and completes it.

Usage: python3 two_machine_hub_test_node2.py <hub-origin> <pairing-code> <card-id>
  e.g. python3 two_machine_hub_test_node2.py http://192.168.1.143:8787 HK7-3PQ 3f9b...
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
    origin, code, card_id = sys.argv[1], sys.argv[2], sys.argv[3]
    creds = call(origin, "/local/v1/pair", {"code": code, "name": "overgaard-two-machine-test"})
    print("Paired as node:", creds["node_id"])
    session = str(uuid.uuid4())
    caps = {
        "hardware": {"cpu_model": "overgaard", "cpu_cores": 1, "ram_bytes": 1, "gpu_vendor": "none", "disk_free_bytes": 1},
        "modalities": ["text"],
        "models": [],
        "allow_internet": False,
        "tools_level": "inference_only",
    }
    rpc(origin, creds["raw_key"], session, "check_in", {"caps": caps, "region": None})
    claim = rpc(origin, creds["raw_key"], session, "claim_card")
    print("Overgaard claim_card:", claim)
    assert claim.get("status") == "leased", f"expected the handed-off card, got: {claim}"
    assert claim["checkpoint"]["state"]["from"] == "midgaard over real LAN", "checkpoint state didn't survive the handoff!"
    usage = {"tokens_in": 1, "tokens_out": 1, "compute_seconds": 0}
    result = rpc(origin, creds["raw_key"], session, "complete_card", {
        "card_id": card_id, "content": "completed on Overgaard, over the real LAN", "model_id": None, "usage": usage,
    })
    print("Overgaard complete_card:", result)
    print("PASS (node2/Overgaard): paired, claimed the checkpointed handoff, completed -- over the real network.")


if __name__ == "__main__":
    main()
