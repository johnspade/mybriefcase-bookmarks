# Enforce post-mutation invariants via Mutator<N: Notify>

The core crate enforces post-mutation invariants (export to shared storage, then notify listeners) via a generic `Mutator<N: Notify>` struct rather than leaving the `export_doc_to_shared` call to convention. We chose compile-time enforcement because forgetting the export step causes silent data loss for peers, and every future consumer (Android/UniFFI, CLI) must honor the same invariant. The alternative — a standalone function that each consumer calls manually — was rejected because it relies on discipline rather than the type system.
