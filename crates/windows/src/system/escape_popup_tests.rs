use std::cell::Cell;
use std::time::Duration;

use agent_desktop_core::{Deadline, DeliverySemantics, ErrorCode};

use super::{
    MAX_POPUP_CHILDREN, MAX_POPUP_DEPTH, MAX_POPUP_NODES, MAX_POPUP_ROOTS, PopupScan, PopupTree,
    opens_a_popup, scan_popup_owners, scan_timeout_error,
};

struct FakeNode {
    open_popup: bool,
    children: Vec<usize>,
}

struct FakeTree {
    nodes: Vec<FakeNode>,
    reads: Cell<usize>,
}

impl FakeTree {
    fn new(nodes: Vec<FakeNode>) -> Self {
        Self {
            nodes,
            reads: Cell::new(0),
        }
    }
}

impl PopupTree for FakeTree {
    type Node = (usize, Option<(usize, usize)>);

    fn is_open_popup_owner(&self, node: &Self::Node) -> bool {
        self.reads.set(self.reads.get() + 1);
        self.nodes[node.0].open_popup
    }

    fn first_child(&self, node: &Self::Node) -> Option<Self::Node> {
        self.nodes[node.0]
            .children
            .first()
            .copied()
            .map(|child| (child, Some((node.0, 0))))
    }

    fn next_sibling(&self, node: &Self::Node) -> Option<Self::Node> {
        let (parent, index) = node.1?;
        let next = index + 1;
        self.nodes[parent]
            .children
            .get(next)
            .copied()
            .map(|child| (child, Some((parent, next))))
    }
}

fn root_ref(id: usize) -> (usize, Option<(usize, usize)>) {
    (id, None)
}

#[test]
fn a_tree_item_or_a_group_is_never_a_popup_owner() {
    assert!(opens_a_popup(50011) && opens_a_popup(50003) && opens_a_popup(50031));
    assert!(!opens_a_popup(50024) && !opens_a_popup(50026) && !opens_a_popup(50029));
}

#[test]
fn popup_scan_stops_immediately_when_deadline_is_expired() {
    let tree = FakeTree::new(vec![FakeNode {
        open_popup: true,
        children: Vec::new(),
    }]);
    let deadline = Deadline::after(1).expect("short deadline");
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(
        scan_popup_owners(&tree, vec![root_ref(0)], deadline),
        PopupScan::TimedOut
    );
    assert_eq!(tree.reads.get(), 0);
    let error = scan_timeout_error(deadline);
    assert_eq!(error.code, ErrorCode::Timeout);
    assert_eq!(error.disposition, DeliverySemantics::not_delivered());
}

#[test]
fn popup_scan_respects_node_child_depth_and_root_budgets() {
    let mut wide = vec![FakeNode {
        open_popup: false,
        children: (1..=MAX_POPUP_CHILDREN + 20).collect(),
    }];
    for idx in 1..=MAX_POPUP_CHILDREN + 20 {
        wide.push(FakeNode {
            open_popup: idx == MAX_POPUP_CHILDREN + 20,
            children: Vec::new(),
        });
    }
    let wide_tree = FakeTree::new(wide);
    let deadline = Deadline::after(5_000).expect("budget");
    assert_eq!(
        scan_popup_owners(&wide_tree, vec![root_ref(0)], deadline),
        PopupScan::None
    );
    assert!(wide_tree.reads.get() <= MAX_POPUP_CHILDREN + 1);

    let mut deep = Vec::new();
    for depth in 0..=MAX_POPUP_DEPTH + 4 {
        deep.push(FakeNode {
            open_popup: depth == MAX_POPUP_DEPTH + 4,
            children: if depth < MAX_POPUP_DEPTH + 4 {
                vec![depth + 1]
            } else {
                Vec::new()
            },
        });
    }
    let deep_tree = FakeTree::new(deep);
    assert_eq!(
        scan_popup_owners(&deep_tree, vec![root_ref(0)], deadline),
        PopupScan::None
    );
    assert_eq!(deep_tree.reads.get(), MAX_POPUP_DEPTH + 1);

    let many_roots: Vec<FakeNode> = (0..MAX_POPUP_ROOTS + 5)
        .map(|idx| FakeNode {
            open_popup: idx == MAX_POPUP_ROOTS + 4,
            children: Vec::new(),
        })
        .collect();
    let root_tree = FakeTree::new(many_roots);
    let roots = (0..MAX_POPUP_ROOTS + 5).map(root_ref).collect();
    assert_eq!(
        scan_popup_owners(&root_tree, roots, deadline),
        PopupScan::None
    );
    assert_eq!(root_tree.reads.get(), MAX_POPUP_ROOTS);

    let mut dense = Vec::new();
    let mut dense_roots = Vec::new();
    for _ in 0..20usize {
        let base = dense.len();
        dense_roots.push(root_ref(base));
        let children: Vec<usize> = (base + 1..base + 51).collect();
        dense.push(FakeNode {
            open_popup: false,
            children,
        });
        for _ in 0..50 {
            dense.push(FakeNode {
                open_popup: false,
                children: Vec::new(),
            });
        }
    }
    let dense_tree = FakeTree::new(dense);
    assert_eq!(
        scan_popup_owners(&dense_tree, dense_roots, deadline),
        PopupScan::None
    );
    assert_eq!(dense_tree.reads.get(), MAX_POPUP_NODES);
}

#[test]
fn popup_scan_returns_the_deepest_open_owner_within_budget() {
    let tree = FakeTree::new(vec![
        FakeNode {
            open_popup: false,
            children: vec![1, 3],
        },
        FakeNode {
            open_popup: true,
            children: vec![2],
        },
        FakeNode {
            open_popup: true,
            children: Vec::new(),
        },
        FakeNode {
            open_popup: true,
            children: Vec::new(),
        },
    ]);
    let deadline = Deadline::after(5_000).expect("budget");
    assert_eq!(
        scan_popup_owners(&tree, vec![root_ref(0)], deadline),
        PopupScan::Found((2, Some((1, 0))))
    );
}
