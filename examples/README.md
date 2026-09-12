# Example configuration

Copy [rustsyncignore.example](rustsyncignore.example) to `.rustsyncignore` in
your workspace before its first sync. Review the patterns for your files;
already tracked paths stay tracked when ignore rules change.

Executable examples live in [scripts/](../scripts/README.md). Run the complete
two-device walkthrough with `scripts/demo.sh` from the repository root.
The canonical container configuration is [compose.yaml](../compose.yaml).
See [deployment](../docs/deployment.md) for running a persistent server.
