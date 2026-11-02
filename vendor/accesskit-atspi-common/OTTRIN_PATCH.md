# Ottrin AT-SPI state correction

Origin: crates.io accesskit_atspi_common 0.14.2, AccessKit commit `5801c4a49ff9b2c9ec3f77b3c31272d0da4c5cd3`.

Backport the disabled-node guard present in upstream `f3026222559c7f5f46d8f25cb7404de68eb149f0`,
`adapters/atspi-common/src/node.rs`. The older backend otherwise reports disabled
buttons as Enabled and Sensitive. Preserve the existing compatible API and all
other mappings. Native before/after evidence is in
`.hermes/evidence/assistive-control-names-current/`.

Remove this patch when the supported egui dependency graph incorporates the
upstream correction. The crate is the sole AT-SPI mapping implementation.
