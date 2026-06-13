use autosurgeon::hydrate;
use mybriefcase_bookmarks::model::BookmarkStore;
use proptest::prelude::*;

use crate::common::{fork_doc, merge_docs, new_initialized_doc};
use crate::invariants::assert_stores_converged;
use crate::strategies::{DocState, arb_op_sequence};

fn hydrate_store(doc: &automerge_repo::DocHandle) -> BookmarkStore {
    doc.with_doc(|d| hydrate(d).unwrap())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, max_shrink_iters: 2048, .. ProptestConfig::default() })]

    #[test]
    fn two_peer_convergence(
        ops_a in arb_op_sequence(3..15),
        ops_b in arb_op_sequence(3..15),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        let peer_a = new_initialized_doc("prop-a");
        let peer_b = fork_doc(&peer_a, "prop-b");

        let mut state_a = DocState::new(peer_a.root_folder_id.clone());
        let mut state_b = DocState::new(peer_b.root_folder_id.clone());

        for op in &ops_a {
            state_a.apply(&peer_a.doc_handle, op);
        }
        for op in &ops_b {
            state_b.apply(&peer_b.doc_handle, op);
        }

        merge_docs(&peer_a.doc_handle, &peer_b.doc_handle);
        merge_docs(&peer_b.doc_handle, &peer_a.doc_handle);

        let store_a = hydrate_store(&peer_a.doc_handle);
        let store_b = hydrate_store(&peer_b.doc_handle);
        assert_stores_converged(&store_a, &store_b);
    }

    #[test]
    fn three_peer_convergence(
        ops_a in arb_op_sequence(2..10),
        ops_b in arb_op_sequence(2..10),
        ops_c in arb_op_sequence(2..10),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        let peer_a = new_initialized_doc("prop-a");
        let peer_b = fork_doc(&peer_a, "prop-b");
        let peer_c = fork_doc(&peer_a, "prop-c");

        let mut state_a = DocState::new(peer_a.root_folder_id.clone());
        let mut state_b = DocState::new(peer_b.root_folder_id.clone());
        let mut state_c = DocState::new(peer_c.root_folder_id.clone());

        for op in &ops_a { state_a.apply(&peer_a.doc_handle, op); }
        for op in &ops_b { state_b.apply(&peer_b.doc_handle, op); }
        for op in &ops_c { state_c.apply(&peer_c.doc_handle, op); }

        // Full mesh merge
        merge_docs(&peer_b.doc_handle, &peer_a.doc_handle);
        merge_docs(&peer_c.doc_handle, &peer_a.doc_handle);
        merge_docs(&peer_a.doc_handle, &peer_b.doc_handle);
        merge_docs(&peer_c.doc_handle, &peer_b.doc_handle);
        merge_docs(&peer_a.doc_handle, &peer_c.doc_handle);
        merge_docs(&peer_b.doc_handle, &peer_c.doc_handle);

        let store_a = hydrate_store(&peer_a.doc_handle);
        let store_b = hydrate_store(&peer_b.doc_handle);
        let store_c = hydrate_store(&peer_c.doc_handle);
        assert_stores_converged(&store_a, &store_b);
        assert_stores_converged(&store_b, &store_c);
    }

    #[test]
    fn merge_order_independence(
        ops_a in arb_op_sequence(3..12),
        ops_b in arb_op_sequence(3..12),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        let base = new_initialized_doc("prop-base");
        let peer_a = fork_doc(&base, "prop-a");
        let peer_b = fork_doc(&base, "prop-b");

        let mut state_a = DocState::new(peer_a.root_folder_id.clone());
        let mut state_b = DocState::new(peer_b.root_folder_id.clone());

        for op in &ops_a { state_a.apply(&peer_a.doc_handle, op); }
        for op in &ops_b { state_b.apply(&peer_b.doc_handle, op); }

        // Merge order 1: A into fresh-from-B
        let view1 = fork_doc(&peer_b, "view1");
        merge_docs(&peer_a.doc_handle, &view1.doc_handle);

        // Merge order 2: B into fresh-from-A
        let view2 = fork_doc(&peer_a, "view2");
        merge_docs(&peer_b.doc_handle, &view2.doc_handle);

        let store1 = hydrate_store(&view1.doc_handle);
        let store2 = hydrate_store(&view2.doc_handle);
        assert_stores_converged(&store1, &store2);
    }

    #[test]
    fn merge_is_idempotent(
        ops in arb_op_sequence(3..15),
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        let peer_a = new_initialized_doc("prop-a");
        let peer_b = fork_doc(&peer_a, "prop-b");

        let mut state_a = DocState::new(peer_a.root_folder_id.clone());
        for op in &ops { state_a.apply(&peer_a.doc_handle, op); }

        merge_docs(&peer_a.doc_handle, &peer_b.doc_handle);
        let store_after_first = hydrate_store(&peer_b.doc_handle);

        merge_docs(&peer_a.doc_handle, &peer_b.doc_handle);
        let store_after_second = hydrate_store(&peer_b.doc_handle);

        assert_stores_converged(&store_after_first, &store_after_second);
    }
}
