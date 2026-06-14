# MyBriefcase Bookmarks

A local-first bookmark manager where each device runs its own server, using Automerge CRDTs for conflict-free sync over Syncthing.

## Language

**Document**:
The single Automerge document holding all bookmark data for one logical store.
_Avoid_: File, database, state

**Client**:
A device instance identified by a unique client_id; writes only its own sync files.
_Avoid_: Node, peer, device

**Sync Root**:
The Syncthing-shared directory where client export files and shared data (favicons) live.
_Avoid_: Data dir, sync folder

**Export (sync)**:
Writing the local document state to a file in sync_root for other clients to merge.
_Avoid_: Save, persist, flush

**Mutation**:
Any write to the Automerge document, always followed by export and notification.
_Avoid_: Update, change, write
