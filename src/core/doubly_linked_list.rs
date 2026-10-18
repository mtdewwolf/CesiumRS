//! CesiumJS `DoublyLinkedList`, with nodes kept in an arena and addressed by
//! [`NodeId`] instead of object references.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(usize);

#[derive(Clone, Debug)]
struct Node<T> {
    item: T,
    previous: Option<NodeId>,
    next: Option<NodeId>,
}

#[derive(Clone, Debug)]
pub struct DoublyLinkedList<T> {
    nodes: Vec<Option<Node<T>>>,
    head: Option<NodeId>,
    tail: Option<NodeId>,
    length: usize,
}

impl<T> Default for DoublyLinkedList<T> {
    fn default() -> Self {
        Self { nodes: Vec::new(), head: None, tail: None, length: 0 }
    }
}

impl<T> DoublyLinkedList<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.length
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub fn head(&self) -> Option<NodeId> {
        self.head
    }

    pub fn tail(&self) -> Option<NodeId> {
        self.tail
    }

    fn node(&self, id: NodeId) -> &Node<T> {
        self.nodes[id.0].as_ref().expect("node was removed")
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node<T> {
        self.nodes[id.0].as_mut().expect("node was removed")
    }

    pub fn item(&self, id: NodeId) -> &T {
        &self.node(id).item
    }

    pub fn item_mut(&mut self, id: NodeId) -> &mut T {
        &mut self.node_mut(id).item
    }

    pub fn next(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).next
    }

    pub fn previous(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).previous
    }

    /// Appends `item` and returns its node.
    pub fn add(&mut self, item: T) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(Some(Node { item, previous: self.tail, next: None }));
        match self.tail {
            Some(tail) => self.node_mut(tail).next = Some(id),
            None => self.head = Some(id),
        }
        self.tail = Some(id);
        self.length += 1;
        id
    }

    fn unlink(&mut self, id: NodeId) {
        let (previous, next) = {
            let n = self.node(id);
            (n.previous, n.next)
        };
        match (previous, next) {
            (Some(p), Some(n)) => {
                self.node_mut(p).next = Some(n);
                self.node_mut(n).previous = Some(p);
            }
            (Some(p), None) => {
                self.node_mut(p).next = None;
                self.tail = Some(p);
            }
            (None, Some(n)) => {
                self.node_mut(n).previous = None;
                self.head = Some(n);
            }
            (None, None) => {
                self.head = None;
                self.tail = None;
            }
        }
        let n = self.node_mut(id);
        n.next = None;
        n.previous = None;
    }

    /// Removes a node and returns its item.
    pub fn remove(&mut self, id: NodeId) -> T {
        self.unlink(id);
        self.length -= 1;
        self.nodes[id.0].take().expect("node was removed").item
    }

    /// Moves `next_node` to directly after `node`.
    pub fn splice(&mut self, node: NodeId, next_node: NodeId) {
        if node == next_node {
            return;
        }
        self.unlink(next_node);
        let old_node_next = self.node(node).next;
        self.node_mut(node).next = Some(next_node);
        if self.tail == Some(node) {
            self.tail = Some(next_node);
        } else if let Some(o) = old_node_next {
            self.node_mut(o).previous = Some(next_node);
        }
        let n = self.node_mut(next_node);
        n.next = old_node_next;
        n.previous = Some(node);
    }

    /// Items from head to tail.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        let mut current = self.head;
        std::iter::from_fn(move || {
            let id = current?;
            current = self.node(id).next;
            Some(&self.node(id).item)
        })
    }
}
