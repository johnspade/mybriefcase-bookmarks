# Export failure is hard failure

The shared store (Syncthing folder) is the source of truth — the local repo store is a rebuildable working copy. If `export_doc_to_shared` fails after a mutation, the operation is reported as failed to the caller even though the local Automerge document was updated. This means `mutate()` propagates export errors identically to operation errors.

Most local-first apps treat sync as best-effort (log and continue), but our architecture writes exclusively to `<client_id>/store/` in the shared folder — there is no other durable path for changes to reach peers. A silently failed export is silent data loss from the perspective of every other device. The local document diverges from what peers can see, and if the device re-initialises from the shared folder, the buffered local change is gone.

## Considered alternatives

- **Log and degrade** — export fails silently, surface sync health via a separate UI indicator. Rejected because the user believes their action succeeded while the data is effectively ephemeral.
- **Separate export error type** — distinguish "operation failed" from "operation succeeded but export failed." Rejected because from the user's perspective both mean "your change didn't stick" — the distinction adds complexity without a different recovery path.
