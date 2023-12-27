use image::{GenericImageView, Luma};
use petgraph::visit::{Data, GraphBase, IntoEdgeReferences, NodeIndexable};

use super::PixelCoordinate;

/// Represent an image as a graph where pixels are nodes and edges are created
/// between each pixel and the 8 pixels surrounding it.
///
/// Edge weights are the absolute difference in pixel intensity, converted to
/// 0.0..1.0 floating-point.
pub struct PixelGrid<I>(pub I);

impl<I> GraphBase for PixelGrid<I>
where
    I: GenericImageView<Pixel = Luma<u8>>,
{
    type EdgeId = Edge;
    type NodeId = PixelCoordinate;
}

impl<I> Data for PixelGrid<I>
where
    I: GenericImageView<Pixel = Luma<u8>>,
{
    type NodeWeight = ();
    type EdgeWeight = f32;
}

impl<I> NodeIndexable for PixelGrid<I>
where
    I: GenericImageView<Pixel = Luma<u8>>,
{
    fn node_bound(&self) -> usize {
        self.0.width() as usize * self.0.height() as usize
    }

    fn to_index(&self, node: Self::NodeId) -> usize {
        node.y as usize * self.0.width() as usize + node.x as usize
    }

    fn from_index(&self, i: usize) -> Self::NodeId {
        PixelCoordinate {
            x: (i % self.0.width() as usize) as u32,
            y: (i / self.0.width() as usize) as u32,
        }
    }
}

impl<'a, I> IntoEdgeReferences for &'a PixelGrid<I>
where
    I: GenericImageView<Pixel = Luma<u8>>,
{
    type EdgeRef = EdgeRef<'a, I>;
    type EdgeReferences = std::vec::IntoIter<Self::EdgeRef>;

    fn edge_references(self) -> Self::EdgeReferences {
        let mut output = Vec::new();
        for (x, y, _) in self.0.pixels() {
            let base = PixelCoordinate { x, y };

            if x < self.0.width() - 1 {
                output.push(EdgeRef::new(
                    self,
                    Edge {
                        base,
                        neighbor: Neighbor::Right,
                    },
                ));
            }
            if y < self.0.height() - 1 {
                output.push(EdgeRef::new(
                    self,
                    Edge {
                        base,
                        neighbor: Neighbor::Down,
                    },
                ));
            }
            if x < self.0.width() - 1 && y < self.0.height() - 1 {
                output.push(EdgeRef::new(
                    self,
                    Edge {
                        base,
                        neighbor: Neighbor::DownRight,
                    },
                ));
            }
            if x > 0 && y < self.0.height() - 1 {
                output.push(EdgeRef::new(
                    self,
                    Edge {
                        base,
                        neighbor: Neighbor::DownLeft,
                    },
                ))
            }
        }
        output.into_iter()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Edge {
    base: PixelCoordinate,
    neighbor: Neighbor,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Neighbor {
    Down,
    Right,
    DownRight,
    DownLeft,
}

impl Neighbor {
    fn apply(&self, c: PixelCoordinate) -> PixelCoordinate {
        match self {
            Self::Down => PixelCoordinate { x: c.x, y: c.y + 1 },
            Self::Right => PixelCoordinate { x: c.x + 1, y: c.y },
            Self::DownRight => PixelCoordinate {
                x: c.x + 1,
                y: c.y + 1,
            },
            Self::DownLeft => PixelCoordinate {
                x: c.x - 1,
                y: c.y + 1,
            },
        }
    }
}

pub struct EdgeRef<'a, I> {
    src: &'a PixelGrid<I>,
    edge: Edge,
    weight: f32,
}

impl<'a, I> EdgeRef<'a, I>
where
    I: GenericImageView<Pixel = Luma<u8>>,
{
    pub fn new(src: &'a PixelGrid<I>, edge: Edge) -> Self {
        let target = edge.neighbor.apply(edge.base);
        let a = src.0.get_pixel(edge.base.x, edge.base.y);
        let b = src.0.get_pixel(target.x, target.y);
        let weight = a.0[0].abs_diff(b.0[0]) as f32 / u8::MAX as f32;
        Self { src, edge, weight }
    }
}

impl<'a, I> Clone for EdgeRef<'a, I> {
    fn clone(&self) -> Self {
        Self {
            src: self.src,
            edge: self.edge,
            weight: self.weight,
        }
    }
}

impl<'a, I> Copy for EdgeRef<'a, I> {}

impl<'a, I> petgraph::visit::EdgeRef for EdgeRef<'a, I>
where
    I: GenericImageView<Pixel = Luma<u8>>,
{
    type NodeId = PixelCoordinate;
    type EdgeId = Edge;
    type Weight = f32;

    fn source(&self) -> Self::NodeId {
        self.edge.base
    }

    fn target(&self) -> Self::NodeId {
        self.edge.neighbor.apply(self.edge.base)
    }

    fn weight(&self) -> &Self::Weight {
        &self.weight
    }

    fn id(&self) -> Self::EdgeId {
        self.edge
    }
}
