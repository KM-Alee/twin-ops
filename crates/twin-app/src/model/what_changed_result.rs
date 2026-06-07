use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WhatChangedResult {
    pub since_ns: i64,
    pub since_label: String,
    pub generated_at_ns: i64,
    pub new_nodes: Vec<WhatChangedNode>,
    pub new_edges: Vec<WhatChangedEdge>,
    pub disappeared_nodes: Vec<WhatChangedNode>,
    pub disappeared_edges: Vec<WhatChangedEdge>,
    pub stale_nodes: Vec<WhatChangedNode>,
    pub stale_edges: Vec<WhatChangedEdge>,
    pub changed_nodes: Vec<WhatChangedNodeDelta>,
    pub changed_edges: Vec<WhatChangedEdgeDelta>,
    pub reappeared_nodes: Vec<WhatChangedNode>,
    pub reappeared_edges: Vec<WhatChangedEdge>,
    pub unknowns: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WhatChangedNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WhatChangedEdge {
    pub id: String,
    pub from_node_id: String,
    pub to_node_id: String,
    pub kind: String,
    pub class: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WhatChangedNodeDelta {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WhatChangedEdgeDelta {
    pub id: String,
    pub summary: String,
}
