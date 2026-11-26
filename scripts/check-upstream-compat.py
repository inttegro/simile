"""Compare a pristine upstream CLI with the fork using deterministic fixtures.

Usage: python3 scripts/check-upstream-compat.py UPSTREAM_BINARY FORK_BINARY
Both binaries must use the same Simile version. All writes use a temporary home.
"""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "crates/simile-core/tests/fixtures"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("upstream", type=Path)
    parser.add_argument("candidate", type=Path)
    args = parser.parse_args()
    binaries = [path.resolve(strict=True) for path in (args.upstream, args.candidate)]
    checks = 0
    with tempfile.TemporaryDirectory(prefix="simile-upstream-compat-") as directory:
        root = Path(directory)
        home = root / "home"
        env = {key: value for key, value in os.environ.items() if not key.startswith("SIMILE_")}
        env.update(SIMILE_HOME=str(home), NO_COLOR="1")

        def run(binary, arguments, expected=0, input=None):
            # Fixed argv[0] keeps Clap usage diagnostics identical across binary paths.
            result = subprocess.run(
                ["simile", "--color", "never", "--quiet", "--output", "json", *map(str, arguments)],
                executable=str(binary), cwd=root, env=env, input=input,
                capture_output=True, timeout=30,
            )
            if result.returncode != expected:
                raise AssertionError(f"Unexpected exit for {arguments}: {result.returncode}\n{result.stderr.decode()}")
            if b"\x1b[" in result.stdout:
                raise AssertionError(f"ANSI output in JSON result: {arguments}")
            return result.returncode, result.stdout, result.stderr

        def compare(label, arguments, expected=0, input=None):
            nonlocal checks
            results = [run(binary, arguments, expected, input) for binary in binaries]
            if results[0] != results[1]:
                raise AssertionError(f"CLI mismatch: {label}\nupstream: {results[0]!r}\nfork: {results[1]!r}")
            checks += 1
            print(f"PASS {label}")

        compare("version", ["version"])
        compare("console help", ["console", "--help"])
        for source, modules in [
            ("orders.yaml", ["--module", FIXTURES / "orders.hcl"]),
            ("orders.smithy", []), ("orders.d.ts", []),
        ]:
            artifact = root / "service.similec"
            arguments = ["compile", FIXTURES / source, *modules, "--artifact", artifact]
            outputs = []
            for binary in binaries:
                outputs.append((run(binary, arguments), artifact.read_bytes()))
            assert outputs[0] == outputs[1], f"Compiled artifact differs: {source}"
            checks += 1
            print(f"PASS {source} compilation and artifact bytes")
            compare(f"{source} inspection", ["inspect", artifact])
            compare(f"{source} audit", ["audit", artifact])

        run_order = ["run", FIXTURES / "run-orders.yaml", "--operation-id", "createOrder", "--verbose"]
        for seed in ["0", "7", "18446744073709551615"]:
            compare(f"seeded synthesis {seed}", [*run_order, "--seed", seed])
        valid_request = '{"headers":{"Content-Type":"application/json"},"body":{"product":"fixture","quantity":4}}'
        compare("explicit JSON request", [*run_order, "--seed", "7", "--request", valid_request])
        compare("stdin JSON request", [*run_order, "--seed", "7", "--request", "-"], input=valid_request.encode())
        compare("bodyless operation", ["run", FIXTURES / "run-orders.yaml", "--operation", "GET /health", "--seed", "7", "--verbose"])
        compare("optional form request", ["run", FIXTURES / "run-orders.yaml", "--operation-id", "optionalForm", "--seed", "7", "--verbose"])
        compare("required query synthesis", ["run", FIXTURES / "run-parameters.yaml", "--operation-id", "openLink", "--seed", "7", "--verbose"])
        for label, request in [
            ("malformed JSON", "{"),
            ("invalid request schema", '{"body":{"product":"fixture","quantity":0}}'),
            ("missing required body", "{}"),
        ]:
            compare(label, [*run_order, "--seed", "7", "--request", request], expected=1)
        compare("unknown operation", ["run", FIXTURES / "run-orders.yaml", "--operation-id", "absent", "--seed", "7"], expected=1)
        compare("missing contract", ["compile", root / "missing.yaml"], expected=1)

        cases = root / "cases"
        bundle = cases / "create-order"
        captured = []
        for binary in binaries:
            shutil.rmtree(cases, ignore_errors=True)
            result = run(binary, [*run_order, "--seed", "7", "--bundle", bundle])
            captured.append((result, {file.name: file.read_bytes() for file in bundle.iterdir()}))
        assert captured[0] == captured[1], "Captured reproduction bundle differs"
        checks += 1
        print("PASS reproduction bundle bytes")
        compare("replay", ["replay", bundle])
        compare("conformance", ["conformance", cases])

        project = root / "project"
        project.mkdir()
        config = project / "Simfile"
        config.write_bytes((FIXTURES / "service-orders.hcl").read_bytes())
        for name in ["run-orders.yaml", "orders.hcl"]:
            (project / name).write_bytes((FIXTURES / name).read_bytes())
        compare("transient description", ["describe", "service", "--config", config])
        registrations = []
        for binary in binaries:
            shutil.rmtree(home, ignore_errors=True)
            result = run(binary, ["register", "orders", "--config", config])
            report = json.loads(result[1])
            revision = report["revision"]
            stored = Path(report["directory"])
            assert (stored / "contract.source").read_bytes() == (FIXTURES / "run-orders.yaml").read_bytes()
            # Only the generated registry revision is normalized; content and all
            # other JSON fields, diagnostics, and exit codes must match.
            normalized = result[1].replace(revision.encode(), b"<revision>")
            registrations.append((result[0], normalized, result[2], (stored / "service.similec").read_bytes()))
        assert registrations[0] == registrations[1], "Registered artifact or metadata differs"
        checks += 1
        print("PASS registration, immutable contract source, and artifact bytes")
        # Both implementations must read a registry created by the fork even if
        # its original contract is subsequently edited or removed.
        (project / "run-orders.yaml").unlink()
        compare("registered description without original source", ["describe", "service", "orders"])
        # Verify the opposite direction too, with the original source restored.
        (project / "run-orders.yaml").write_bytes((FIXTURES / "run-orders.yaml").read_bytes())
        compare("duplicate registration diagnostic", ["register", "orders", "--config", config], expected=1)
        shutil.rmtree(home)
        run(binaries[0], ["register", "orders", "--config", config])
        compare("reading upstream registration", ["describe", "service", "orders"])
    print(f"{checks} upstream/fork compatibility checks passed")


if __name__ == "__main__":
    main()
