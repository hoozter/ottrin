# Configured Windows Copy registry and creation-time recovery

See `ACCEPTANCE.json` for source hashes, exact native executable pins, test scope,
remaining release gates and cleanup proof. `FINAL_GUEST.json` and
`FINAL_CLEANUP.json` prove original state preservation and owned VM disposal.

`SCHEMA11_FULL_*` records the full platform suite. `SCHEMA11_CODEC_*` and
`SCHEMA11_DRIFT_*` record later test-only additions against distinct executable
pins. `SCHEMA11_UI_*`, `SCHEMA11_ui.*` and `SCHEMA11_audit.*` record schema-11 UI
and explicit SYSTEM audit checks. `RED_*`, the initial failed full-suite receipts,
and native tunneling probes preserve causal evidence and fixture corrections.

Public forward Windows Copy remains gated; packages are unchanged. No physical
power-cut, broad storage/security, release or user acceptance is implied.
Disposable transfer controls, build transcripts, test scripts, VM and disk were
removed after their consumers exited. Shared build output and source VM backing
images were retained.
