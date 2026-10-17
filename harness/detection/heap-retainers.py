#!/usr/bin/env python3
"""Find native root paths to objects marked by an own diagnostic property."""
import argparse
from collections import defaultdict, deque
import hashlib
import json
from pathlib import Path


def root_path(target, reverse, label):
    pending = deque([target])
    visited = {target}
    steps = {}
    while pending:
        node = pending.popleft()
        if node == 0:
            result = []
            while node != target:
                child, kind, name = steps[node]
                result.append({"from": label(node), "type": kind, "edge": name, "to": label(child)})
                node = child
            return result
        for source, kind, name in reverse[node]:
            if source not in visited:
                visited.add(source)
                steps[source] = (node, kind, name)
                pending.append(source)
    return None


def analyze(heap, marker, omit_kept):
    meta = heap["snapshot"]["meta"]
    nodes, edges, strings = heap["nodes"], heap["edges"], heap["strings"]
    fields, edge_fields = meta["node_fields"], meta["edge_fields"]
    node_width, edge_width = len(fields), len(edge_fields)
    node_types, edge_types = meta["node_types"][0], meta["edge_types"][0]
    count_offset = fields.index("edge_count")
    type_offset, name_offset, id_offset = (fields.index(name) for name in ["type", "name", "id"])
    edge_type, edge_name, edge_node = (edge_fields.index(name) for name in ["type", "name_or_index", "to_node"])
    reverse, targets = defaultdict(list), []
    position = 0
    for source in range(0, len(nodes), node_width):
        for _ in range(nodes[source + count_offset]):
            kind = edge_types[edges[position + edge_type]]
            name = edges[position + edge_name]
            name = str(name) if kind in ["element", "hidden"] else strings[name]
            destination = edges[position + edge_node]
            # Weak-map pair edges are conditional; treating their value edge as
            # an unconditional root would report a false realm leak.
            conditional = kind == "weak" or "part of key" in name
            kept = omit_kept and name == "weak_refs_keep_during_job"
            if not conditional and not kept:
                reverse[destination].append((source, kind, name))
            if kind == "property" and name == marker:
                targets.append(source)
            position += edge_width

    def label(node):
        return {"type": node_types[nodes[node + type_offset]], "name": strings[nodes[node + name_offset]], "id": nodes[node + id_offset]}

    return [{"target": label(node), "rootPath": root_path(node, reverse, label)} for node in targets]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("snapshot", type=Path)
    parser.add_argument("--marker", default="__nativeLifecycleDiagnostic")
    parser.add_argument("--include-kept-objects", action="store_true")
    args = parser.parse_args()
    source = args.snapshot.read_bytes()
    result = analyze(json.loads(source), args.marker, not args.include_kept_objects)
    print(json.dumps({
        "snapshotSHA256": hashlib.sha256(source).hexdigest(),
        "marker": args.marker,
        "excludedWeakRefJobRetention": not args.include_kept_objects,
        "bounds": "Shortest unconditional paths; ephemeron pair reachability requires separate analysis.",
        "targets": result,
    }, indent=2))


if __name__ == "__main__":
    main()
