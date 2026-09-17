#!/usr/bin/env python3
"""Prepare offline two-machine fixtures. Never submits jobs or contacts a provider."""
import argparse
import json
from pathlib import Path
import uuid


def prepare(root, mid_model, over_model):
    run = str(uuid.uuid4())
    workspace = f"/private/tmp/hive-team-{run}"
    nodes = ["f1cc4f2b-9820-4955-9912-449e2f52d93c", "ce95c14d-db8d-4a88-90c8-277c067fc57c"]
    cases = [
        ("normalize", "Implement normalize_label(text) in normalize.py. Strip surrounding whitespace, collapse interior whitespace to single spaces, and lowercase. Preserve punctuation.", "from normalize import normalize_label as f; assert f('  HELLO   Hive  ') == 'hello hive'; assert f('A\\tB\\nC') == 'a b c'; assert f('   ') == ''; assert f(' Keep-This! ') == 'keep-this!'"),
        ("count", "Implement count_words(text) in count_words.py. Return the number of whitespace-separated words. Empty or whitespace-only text has zero words.", "from count_words import count_words as f; assert f('hello hive') == 2; assert f(' A\\tB\\nC ') == 3; assert f('') == 0; assert f('   ') == 0; assert f('keep-this!') == 1"),
    ]
    children = []
    for (role, task, check), node, model in zip(cases, nodes, [mid_model, over_model]):
        task += " Only change your named Python module. After running the check, include the complete source in your final report so the coordinator can review it without shared storage."
        children.append({"key": f"team-{run}-{role}", "title": f"Team trial: {role}", "modality": "code", "inputs": task,
            "acceptance": "Required host check passes; report includes complete source.",
            "required_capabilities": {"task": task, "workspace_path": workspace, "brain": "local", "model_id": model,
                "target_node_id": node, "tools_level": "sandboxed_tools", "max_turns": 6, "coordinator": False,
                "acceptance": [{"name": role, "command": "/usr/bin/python3", "args": ["-c", check], "required": True, "expect_exit": 0}]}})
    manifest = {"run_id": run, "mode": "PREPARATION_ONLY", "project_title": "Local Fleet Test", "workspace_on_each_host": workspace,
        "coordinator_node": nodes[0], "coordinator_brain": "nous", "coordinator_max_turns_per_lease": 6,
        "children": children, "stop_after_minutes": 10,
        "launch_blockers": ["Deploy and verify the coordinator recovery migration and matching worker; see SIF-COORDINATOR-RESUME-HANDOFF-2026-09-17.md.",
                            "Integrated fleet build and exact installed local model IDs must be verified.",
                            "Total coordinator leases/paid turns need an operator-enforced cap; max_turns only bounds each session."]}
    out = root / run
    out.mkdir(parents=True, exist_ok=False)
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    prompt = f"""Two-machine trial {run}. You coordinate; local agents implement the modules.
Create exactly the two child jobs in children.json using spawn_card. Do not implement their work yourself.
Create BOTH children before calling wait_for_child: waiting ends this session immediately.
Do not recreate children after restart. Use trusted completed-child context to determine prior work.
After both pass their host checks, review both source reports and explain how normalize_label and count_words compose.
Write final-review.json with run_id, child card IDs, each review verdict and a worked example:
input '  HELLO   Hive  ', normalized 'hello hive', word_count 2.
Never claim completion from a child's prose alone. Missing child context or acceptance evidence means blocked, not success.
If you cannot retrieve child results on resume, report that limitation and stop; do not spawn duplicates.
"""
    (out / "coordinator-task.txt").write_text(prompt)
    (out / "children.json").write_text(json.dumps(children, indent=2) + "\n")
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--midgaard-model", required=True, help="Exact locally installed model ID; confirm before live use")
    parser.add_argument("--overgaard-model", required=True, help="Exact locally installed model ID; confirm before live use")
    args = parser.parse_args()
    for model in [args.midgaard_model, args.overgaard_model]:
        if not model.strip():
            parser.error("model IDs must not be blank")
    print(prepare(args.output, args.midgaard_model, args.overgaard_model))


if __name__ == "__main__":
    main()
