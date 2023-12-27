//! Graph-based image segmentation.
//!
//! Implemented based on the paper "Efficient Graph-Based Image Segmentation" by
//! Felzenszwalb and Huttenlocher (2004)

pub mod pixel_grid;

pub use pixel_grid::PixelGrid;

use petgraph::visit::{Data, EdgeRef, GraphBase, IntoEdgeReferences, NodeIndexable};

#[derive(Clone)]
pub struct Component {
    pub int_diff: f32,
    pub node_count: usize,
}

#[derive(Clone)]
enum ComponentSlot {
    Here(Component),
    There(usize),
}

#[non_exhaustive]
pub struct Segmentation {
    /// Map from node indexes to component indexes
    pub node_components: Vec<usize>,

    /// Component metadata from the segmentation algorithm
    pub components: Vec<Component>,
}

pub fn segment<G>(graph: &G, k: f32) -> Segmentation
where
    G: GraphBase + Data<EdgeWeight = f32> + NodeIndexable + IntoEdgeReferences,
{
    // Component storage, and also mapping node indexes to components.
    let mut components = vec![
        ComponentSlot::Here(Component {
            int_diff: 0.0,
            node_count: 1
        });
        graph.node_bound()
    ];

    // To make merging easier, a component slot may point to another index via
    // `ComponentSlot::There`. To find the component that a node currently
    // belongs to, just follow the indexes until an instance of
    // `ComponentSlot::Here` is found.
    fn get_component(components: &[ComponentSlot], mut idx: usize) -> (usize, &Component) {
        loop {
            match &components[idx] {
                ComponentSlot::Here(component) => {
                    break (idx, component);
                }
                ComponentSlot::There(new_idx) => {
                    idx = *new_idx;
                }
            }
        }
    }

    // Sort E by non-decreasing edge weight.
    let mut queue: Vec<G::EdgeRef> = graph.edge_references().collect();
    queue.sort_by(|a, b| {
        a.weight()
            .partial_cmp(b.weight())
            .expect("NaN encountered in edge weights")
    });

    for edge in queue {
        // Let v1 and v2 denote the vertices connected by the edge.
        let v1_idx = graph.to_index(edge.source());
        let v2_idx = graph.to_index(edge.target());

        let (c1_idx, c1) = get_component(&components, v1_idx);
        let (c2_idx, c2) = get_component(&components, v2_idx);

        // If v1 and v2 are in disjoint components and the edge weight is small
        // compared to the internal difference of both components, then merge
        // the two components, otherwise do nothing.
        if c1_idx == c2_idx {
            continue;
        }
        // TODO customizable threshold function
        let mint = f32::min(
            c1.int_diff + k / (c1.node_count as f32),
            c2.int_diff + k / (c2.node_count as f32),
        );
        if *edge.weight() > mint {
            continue;
        }

        // Merge the components (c2 into c1).
        let new_component = Component {
            int_diff: c1.int_diff.max(c2.int_diff).max(*edge.weight()),
            node_count: c1.node_count + c2.node_count,
        };
        components[c1_idx] = ComponentSlot::Here(new_component);
        components[c2_idx] = ComponentSlot::There(c1_idx);
    }

    // Gather the remaining components and re-index them.
    let mut component_map: Vec<Option<usize>> = vec![None; components.len()];

    let out_components: Vec<Component> = components
        .iter()
        .enumerate()
        .filter_map(|(i, slot)| match slot {
            ComponentSlot::Here(component) => Some((i, component)),
            ComponentSlot::There(_) => None,
        })
        .enumerate()
        .map(|(i_dst, (i_src, component))| {
            component_map[i_src] = Some(i_dst);
            component.clone()
        })
        .collect();

    let out_node_components = (0..graph.node_bound())
        .map(|node_idx| component_map[get_component(&components, node_idx).0].unwrap())
        .collect();

    Segmentation {
        node_components: out_node_components,
        components: out_components,
    }
}
