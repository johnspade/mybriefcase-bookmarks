use autosurgeon::hydrate;
use mybriefcase_bookmarks::model::BookmarkStore;
use mybriefcase_bookmarks::ops;
use proptest::prelude::*;

use crate::common::{fork_doc, merge_docs, new_initialized_doc};
use crate::invariants::{
    assert_cascade_complete, assert_stores_converged, assert_structural_integrity,
    assert_valid_tree,
};
use crate::strategies::{DocState, arb_op, arb_op_sequence};

fn hydrate_store(doc: &automerge_repo::DocHandle) -> BookmarkStore {
    doc.with_doc(|d| hydrate(d).unwrap())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, max_shrink_iters: 2048, .. ProptestConfig::default() })]

    #[test]
    #[cfg_attr(miri, ignore)]
    fn moves_preserve_tree_validity(ops in arb_op_sequence(5..20)) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        let doc = new_initialized_doc("prop-move");
        let mut state = DocState::new(doc.root_folder_id.clone());

        for op in &ops {
            state.apply(&doc.doc_handle, op);
        }

        let store = hydrate_store(&doc.doc_handle);
        assert_valid_tree(&store);
        assert_structural_integrity(&store);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn delete_cascade_descendants(
        setup_ops in arb_op_sequence(5..15),
        folder_idx in 0..10usize,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        let doc = new_initialized_doc("prop-del");
        let mut state = DocState::new(doc.root_folder_id.clone());

        for op in &setup_ops {
            state.apply(&doc.doc_handle, op);
        }

        // Delete a non-root folder if one exists
        if state.folder_ids.len() > 1 {
            let idx = 1 + (folder_idx % (state.folder_ids.len() - 1));
            let target_id = state.folder_ids[idx].clone();
            let _ = ops::delete_folder(&doc.doc_handle, &target_id);

            let store = hydrate_store(&doc.doc_handle);
            assert_cascade_complete(&store, &target_id);
            assert_structural_integrity(&store);
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn delete_preserves_unrelated(
        setup_ops in arb_op_sequence(5..15),
        folder_idx in 0..10usize,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        let doc = new_initialized_doc("prop-del-unrel");
        let mut state = DocState::new(doc.root_folder_id.clone());

        for op in &setup_ops {
            state.apply(&doc.doc_handle, op);
        }

        if state.folder_ids.len() > 1 {
            let idx = 1 + (folder_idx % (state.folder_ids.len() - 1));
            let target_id = state.folder_ids[idx].clone();

            let store_before = hydrate_store(&doc.doc_handle);

            // Collect all descendants of the target folder
            let descendants = collect_descendants(&store_before, &target_id);

            let _ = ops::delete_folder(&doc.doc_handle, &target_id);
            let store_after = hydrate_store(&doc.doc_handle);

            // Items not in the subtree must retain their deleted state
            for (id, bm) in &store_before.bookmarks {
                if !descendants.contains(id) {
                    let after = &store_after.bookmarks[id];
                    prop_assert_eq!(
                        bm.deleted, after.deleted,
                        "unrelated bookmark {} changed deleted state", id
                    );
                }
            }
            for (id, f) in &store_before.folders {
                if !descendants.contains(id) && id != &target_id {
                    let after = &store_after.folders[id];
                    prop_assert_eq!(
                        f.deleted, after.deleted,
                        "unrelated folder {} changed deleted state", id
                    );
                }
            }
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn concurrent_moves_converge(
        setup_ops in arb_op_sequence(5..12),
        move_item_idx in 0..20usize,
        to_a_idx in 0..10usize,
        to_b_idx in 0..10usize,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        let peer_a = new_initialized_doc("prop-mv-a");
        let mut state = DocState::new(peer_a.root_folder_id.clone());

        for op in &setup_ops {
            state.apply(&peer_a.doc_handle, op);
        }

        let peer_b = fork_doc(&peer_a, "prop-mv-b");

        // Both peers attempt a move of the same item to different targets
        let all_ids: Vec<_> = state.folder_ids.iter()
            .chain(state.bookmark_ids.iter())
            .cloned()
            .collect();

        if !all_ids.is_empty() && state.folder_ids.len() > 1 {
            let item_id = &all_ids[move_item_idx % all_ids.len()];
            let from_id = &state.folder_ids[0]; // root
            let to_a = &state.folder_ids[to_a_idx % state.folder_ids.len()];
            let to_b = &state.folder_ids[to_b_idx % state.folder_ids.len()];

            let _ = ops::move_item(&peer_a.doc_handle, item_id, from_id, to_a);
            let _ = ops::move_item(&peer_b.doc_handle, item_id, from_id, to_b);

            merge_docs(&peer_a.doc_handle, &peer_b.doc_handle);
            merge_docs(&peer_b.doc_handle, &peer_a.doc_handle);

            let store_a = hydrate_store(&peer_a.doc_handle);
            let store_b = hydrate_store(&peer_b.doc_handle);
            assert_stores_converged(&store_a, &store_b);
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn concurrent_delete_and_add(
        setup_ops in arb_op_sequence(3..10),
        extra_op in arb_op(),
        folder_idx in 0..10usize,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        let peer_a = new_initialized_doc("prop-da-a");
        let mut state_a = DocState::new(peer_a.root_folder_id.clone());

        for op in &setup_ops {
            state_a.apply(&peer_a.doc_handle, op);
        }

        let peer_b = fork_doc(&peer_a, "prop-da-b");
        let mut state_b = DocState::new(peer_b.root_folder_id.clone());
        state_b.folder_ids = state_a.folder_ids.clone();
        state_b.bookmark_ids = state_a.bookmark_ids.clone();

        // Peer A: delete a folder
        if state_a.folder_ids.len() > 1 {
            let idx = 1 + (folder_idx % (state_a.folder_ids.len() - 1));
            let target_id = state_a.folder_ids[idx].clone();
            let _ = ops::delete_folder(&peer_a.doc_handle, &target_id);

            // Peer B: do another operation
            state_b.apply(&peer_b.doc_handle, &extra_op);

            merge_docs(&peer_a.doc_handle, &peer_b.doc_handle);
            merge_docs(&peer_b.doc_handle, &peer_a.doc_handle);

            let store_a = hydrate_store(&peer_a.doc_handle);
            let store_b = hydrate_store(&peer_b.doc_handle);
            assert_stores_converged(&store_a, &store_b);
        }
    }
}

fn collect_descendants(
    store: &BookmarkStore,
    folder_id: &str,
) -> std::collections::HashSet<String> {
    let mut result = std::collections::HashSet::new();
    let mut stack = vec![folder_id.to_string()];
    while let Some(fid) = stack.pop() {
        if let Some(f) = store.folders.get(&fid) {
            for child_id in &f.children {
                result.insert(child_id.clone());
                if store.folders.contains_key(child_id) {
                    stack.push(child_id.clone());
                }
            }
        }
    }
    result
}
