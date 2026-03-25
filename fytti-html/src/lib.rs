use html5ever::parse_document;
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::{Attribute, ExpandedName, QualName};
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

// ── DOM Types ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub usize);

#[derive(Debug)]
pub struct Document {
    pub nodes: Vec<Node>,
}

#[derive(Debug)]
pub struct Node {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub data: NodeData,
}

#[derive(Debug)]
pub enum NodeData {
    Document,
    Element(ElementData),
    Text(String),
    Comment(String),
    Doctype,
}

#[derive(Debug)]
pub struct ElementData {
    pub name: QualName,
    pub attributes: HashMap<String, String>,
}

impl Document {
    fn new() -> Self {
        let root = Node {
            id: NodeId(0),
            parent: None,
            children: Vec::new(),
            data: NodeData::Document,
        };
        Document { nodes: vec![root] }
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0]
    }

    pub fn root(&self) -> NodeId {
        NodeId(0)
    }

    pub fn tag_name(&self, id: NodeId) -> Option<&str> {
        match &self.nodes[id.0].data {
            NodeData::Element(el) => Some(&el.name.local),
            _ => None,
        }
    }

    pub fn get_attr(&self, id: NodeId, name: &str) -> Option<&str> {
        match &self.nodes[id.0].data {
            NodeData::Element(el) => el.attributes.get(name).map(|s| s.as_str()),
            _ => None,
        }
    }

    pub fn text_content(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.collect_text(id, &mut out);
        out
    }

    fn collect_text(&self, id: NodeId, out: &mut String) {
        match &self.nodes[id.0].data {
            NodeData::Text(t) => out.push_str(t),
            _ => {
                for &child in &self.nodes[id.0].children {
                    self.collect_text(child, out);
                }
            }
        }
    }

    pub fn body(&self) -> NodeId {
        self.find_tag(self.root(), "body").unwrap_or(self.root())
    }

    fn find_tag(&self, from: NodeId, tag: &str) -> Option<NodeId> {
        for &child in &self.nodes[from.0].children {
            if self.tag_name(child) == Some(tag) {
                return Some(child);
            }
            if let Some(found) = self.find_tag(child, tag) {
                return Some(found);
            }
        }
        None
    }

    pub fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut current = self.nodes[id.0].parent;
        while let Some(pid) = current {
            result.push(pid);
            current = self.nodes[pid.0].parent;
        }
        result
    }
}

// ── html5ever TreeSink (uses RefCell for interior mutability) ──

struct Sink {
    doc: RefCell<Document>,
    quirks_mode: Cell<QuirksMode>,
}

impl Sink {
    fn new() -> Self {
        Sink {
            doc: RefCell::new(Document::new()),
            quirks_mode: Cell::new(QuirksMode::NoQuirks),
        }
    }

    fn new_node(&self, data: NodeData) -> NodeId {
        let mut doc = self.doc.borrow_mut();
        let id = NodeId(doc.nodes.len());
        doc.nodes.push(Node {
            id,
            parent: None,
            children: Vec::new(),
            data,
        });
        id
    }

    fn append_child(&self, parent: NodeId, child: NodeId) {
        let mut doc = self.doc.borrow_mut();
        doc.nodes[child.0].parent = Some(parent);
        doc.nodes[parent.0].children.push(child);
    }

    fn remove_child(&self, parent: NodeId, child: NodeId) {
        let mut doc = self.doc.borrow_mut();
        doc.nodes[parent.0].children.retain(|c| *c != child);
        doc.nodes[child.0].parent = None;
    }

    fn detach(&self, target: NodeId) {
        let parent = self.doc.borrow().nodes[target.0].parent;
        if let Some(p) = parent {
            self.remove_child(p, target);
        }
    }
}

impl TreeSink for Sink {
    type Handle = NodeId;
    type Output = Document;
    type ElemName<'a> = ExpandedName<'a>;

    fn finish(self) -> Document {
        self.doc.into_inner()
    }

    fn get_document(&self) -> NodeId {
        NodeId(0)
    }

    fn elem_name<'a>(&'a self, target: &'a NodeId) -> ExpandedName<'a> {
        let doc = self.doc.borrow();
        // SAFETY: we hold the borrow for 'a via the lifetime tie to &self
        // This is sound because the doc nodes are append-only (never removed from vec)
        let node = &doc.nodes[target.0] as *const Node;
        unsafe {
            match &(*node).data {
                NodeData::Element(el) => el.name.expanded(),
                _ => panic!("elem_name called on non-element"),
            }
        }
    }

    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<Attribute>,
        _flags: ElementFlags,
    ) -> NodeId {
        let attributes = attrs
            .into_iter()
            .map(|a| (a.name.local.to_string(), a.value.to_string()))
            .collect();
        self.new_node(NodeData::Element(ElementData { name, attributes }))
    }

    fn create_comment(&self, text: StrTendril) -> NodeId {
        self.new_node(NodeData::Comment(text.to_string()))
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> NodeId {
        self.new_node(NodeData::Comment(String::new()))
    }

    fn append(&self, parent: &NodeId, child: NodeOrText<NodeId>) {
        match child {
            NodeOrText::AppendNode(id) => {
                self.detach(id);
                self.append_child(*parent, id);
            }
            NodeOrText::AppendText(text) => {
                // Merge with last text child if possible
                let last_child = self.doc.borrow().nodes[parent.0].children.last().copied();
                if let Some(last) = last_child {
                    let mut doc = self.doc.borrow_mut();
                    if let NodeData::Text(ref mut existing) = doc.nodes[last.0].data {
                        existing.push_str(&text);
                        return;
                    }
                }
                let id = self.new_node(NodeData::Text(text.to_string()));
                self.append_child(*parent, id);
            }
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &NodeId,
        prev_element: &NodeId,
        child: NodeOrText<NodeId>,
    ) {
        let has_parent = self.doc.borrow().nodes[element.0].parent.is_some();
        if has_parent {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_before_sibling(&self, sibling: &NodeId, child: NodeOrText<NodeId>) {
        let parent = self.doc.borrow().nodes[sibling.0]
            .parent
            .expect("sibling has no parent");

        let child_id = match child {
            NodeOrText::AppendNode(id) => {
                self.detach(id);
                id
            }
            NodeOrText::AppendText(text) => self.new_node(NodeData::Text(text.to_string())),
        };

        let mut doc = self.doc.borrow_mut();
        doc.nodes[child_id.0].parent = Some(parent);
        let siblings = &mut doc.nodes[parent.0].children;
        let pos = siblings.iter().position(|c| *c == *sibling).unwrap_or(0);
        siblings.insert(pos, child_id);
    }

    fn append_doctype_to_document(
        &self,
        _name: StrTendril,
        _public_id: StrTendril,
        _system_id: StrTendril,
    ) {
        let id = self.new_node(NodeData::Doctype);
        self.append_child(NodeId(0), id);
    }

    fn add_attrs_if_missing(&self, target: &NodeId, attrs: Vec<Attribute>) {
        let mut doc = self.doc.borrow_mut();
        if let NodeData::Element(ref mut el) = doc.nodes[target.0].data {
            for attr in attrs {
                let key = attr.name.local.to_string();
                el.attributes.entry(key).or_insert_with(|| attr.value.to_string());
            }
        }
    }

    fn get_template_contents(&self, target: &NodeId) -> NodeId {
        *target
    }

    fn same_node(&self, x: &NodeId, y: &NodeId) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.quirks_mode.set(mode);
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn remove_from_parent(&self, target: &NodeId) {
        self.detach(*target);
    }

    fn reparent_children(&self, node: &NodeId, new_parent: &NodeId) {
        let children: Vec<NodeId> = self.doc.borrow().nodes[node.0].children.clone();
        {
            let mut doc = self.doc.borrow_mut();
            doc.nodes[node.0].children.clear();
            for child in children {
                doc.nodes[child.0].parent = Some(*new_parent);
                doc.nodes[new_parent.0].children.push(child);
            }
        }
    }
}

// ── Public API ──

pub fn parse(html: &str) -> Document {
    let sink = Sink::new();
    parse_document(sink, Default::default())
        .from_utf8()
        .one(html.as_bytes())
}
