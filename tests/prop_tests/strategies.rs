use mybriefcase_bookmarks::import::ImportedItem;
use mybriefcase_bookmarks::ops;
use proptest::prelude::*;

#[derive(Debug, Clone)]
pub enum Op {
    AddBookmark {
        folder_idx: usize,
        url: String,
        title: String,
    },
    CreateFolder {
        parent_idx: usize,
        title: String,
    },
    DeleteBookmark {
        bookmark_idx: usize,
    },
    DeleteFolder {
        folder_idx: usize,
    },
    RenameFolder {
        folder_idx: usize,
        new_title: String,
    },
    MoveItem {
        item_idx: usize,
        from_idx: usize,
        to_idx: usize,
    },
    UpdateBookmark {
        bookmark_idx: usize,
        url: Option<String>,
        title: Option<String>,
        notes: Option<String>,
    },
}

fn arb_url() -> impl Strategy<Value = String> {
    "[a-z]{3,8}".prop_map(|s| format!("https://{s}.example.com"))
}

fn arb_title() -> impl Strategy<Value = String> {
    proptest::collection::vec(proptest::char::range('a', 'z'), 2..12)
        .prop_map(|chars| chars.into_iter().collect())
}

fn arb_notes() -> impl Strategy<Value = String> {
    "[A-Za-z0-9 .]{0,30}"
}

pub fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0..10usize, arb_url(), arb_title())
            .prop_map(|(idx, url, title)| Op::AddBookmark { folder_idx: idx, url, title }),
        3 => (0..10usize, arb_title())
            .prop_map(|(idx, title)| Op::CreateFolder { parent_idx: idx, title }),
        1 => (0..20usize).prop_map(|idx| Op::DeleteBookmark { bookmark_idx: idx }),
        1 => (0..10usize).prop_map(|idx| Op::DeleteFolder { folder_idx: idx }),
        1 => (0..10usize, arb_title())
            .prop_map(|(idx, title)| Op::RenameFolder { folder_idx: idx, new_title: title }),
        1 => (0..20usize, 0..10usize, 0..10usize)
            .prop_map(|(item, from, to)| Op::MoveItem { item_idx: item, from_idx: from, to_idx: to }),
        2 => (0..20usize, proptest::option::of(arb_url()), proptest::option::of(arb_title()), proptest::option::of(arb_notes()))
            .prop_map(|(idx, url, title, notes)| Op::UpdateBookmark { bookmark_idx: idx, url, title, notes }),
    ]
}

pub fn arb_op_sequence(len: std::ops::Range<usize>) -> impl Strategy<Value = Vec<Op>> {
    proptest::collection::vec(arb_op(), len)
}

pub fn arb_imported_item() -> impl Strategy<Value = ImportedItem> {
    let leaf = (arb_url(), arb_title(), arb_notes()).prop_map(|(url, title, notes)| {
        ImportedItem::Bookmark {
            url,
            title,
            notes,
            created_at: None,
            updated_at: None,
        }
    });
    leaf.prop_recursive(4, 64, 8, |inner| {
        (arb_title(), proptest::collection::vec(inner, 0..8)).prop_map(|(title, children)| {
            ImportedItem::Folder {
                title,
                created_at: None,
                updated_at: None,
                children,
            }
        })
    })
}

pub fn arb_imported_tree() -> impl Strategy<Value = Vec<ImportedItem>> {
    proptest::collection::vec(arb_imported_item(), 1..10)
}

pub struct DocState {
    pub folder_ids: Vec<String>,
    pub bookmark_ids: Vec<String>,
}

impl DocState {
    pub fn new(root_folder_id: String) -> Self {
        Self {
            folder_ids: vec![root_folder_id],
            bookmark_ids: Vec::new(),
        }
    }

    pub fn apply(&mut self, doc: &automerge_repo::DocHandle, op: &Op) {
        match op {
            Op::AddBookmark {
                folder_idx,
                url,
                title,
            } => {
                if self.folder_ids.is_empty() {
                    return;
                }
                let folder_id = self.folder_ids[*folder_idx % self.folder_ids.len()].clone();
                if let Ok(id) = ops::add_bookmark(doc, &folder_id, url, title) {
                    self.bookmark_ids.push(id);
                }
            }
            Op::CreateFolder { parent_idx, title } => {
                if self.folder_ids.is_empty() {
                    return;
                }
                let parent = self.folder_ids[*parent_idx % self.folder_ids.len()].clone();
                if let Ok(id) = ops::create_folder(doc, &parent, title) {
                    self.folder_ids.push(id);
                }
            }
            Op::DeleteBookmark { bookmark_idx } => {
                if self.bookmark_ids.is_empty() {
                    return;
                }
                let id = self.bookmark_ids[*bookmark_idx % self.bookmark_ids.len()].clone();
                let _ = ops::delete_bookmark(doc, &id);
            }
            Op::DeleteFolder { folder_idx } => {
                if self.folder_ids.len() <= 1 {
                    return;
                }
                let idx = 1 + (*folder_idx % (self.folder_ids.len() - 1));
                let id = self.folder_ids[idx].clone();
                let _ = ops::delete_folder(doc, &id);
            }
            Op::RenameFolder {
                folder_idx,
                new_title,
            } => {
                if self.folder_ids.is_empty() {
                    return;
                }
                let id = self.folder_ids[*folder_idx % self.folder_ids.len()].clone();
                let _ = ops::rename_folder(doc, &id, new_title);
            }
            Op::MoveItem {
                item_idx,
                from_idx,
                to_idx,
            } => {
                let all_ids: Vec<_> = self
                    .folder_ids
                    .iter()
                    .chain(self.bookmark_ids.iter())
                    .cloned()
                    .collect();
                if all_ids.is_empty() || self.folder_ids.is_empty() {
                    return;
                }
                let item_id = &all_ids[*item_idx % all_ids.len()];
                let from_id = &self.folder_ids[*from_idx % self.folder_ids.len()];
                let to_id = &self.folder_ids[*to_idx % self.folder_ids.len()];
                let _ = ops::move_item(doc, item_id, from_id, to_id);
            }
            Op::UpdateBookmark {
                bookmark_idx,
                url,
                title,
                notes,
            } => {
                if self.bookmark_ids.is_empty() {
                    return;
                }
                let id = self.bookmark_ids[*bookmark_idx % self.bookmark_ids.len()].clone();
                let _ = ops::update_bookmark(
                    doc,
                    &id,
                    url.as_deref(),
                    title.as_deref(),
                    notes.as_deref(),
                );
            }
        }
    }
}
