//! CesiumJS `TexturePacker`: binary atlas subdivision with border padding.

use super::bounding_rectangle::BoundingRectangle;

#[derive(Clone, Debug)]
pub struct TextureNode {
    pub rectangle: BoundingRectangle,
    pub index: Option<usize>,
    pub child_node1: Option<Box<TextureNode>>,
    pub child_node2: Option<Box<TextureNode>>,
}

impl TextureNode {
    fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            rectangle: BoundingRectangle::new(x, y, width, height),
            index: None,
            child_node1: None,
            child_node2: None,
        }
    }

    fn find(&mut self, width: f64, height: f64, padding: f64) -> Option<&mut Self> {
        if self.child_node1.is_none() && self.child_node2.is_none() {
            if self.index.is_some() {
                return None;
            }
            let r = self.rectangle;
            let dw = r.width - width;
            let dh = r.height - height;
            if dw < 0.0 || dh < 0.0 {
                return None;
            }
            if dw == 0.0 && dh == 0.0 {
                return Some(self);
            }
            if dw > dh {
                self.child_node1 = Some(Box::new(Self::new(r.x, r.y, width, r.height)));
                if dw - padding > 0.0 {
                    self.child_node2 = Some(Box::new(Self::new(r.x + width + padding, r.y, dw - padding, r.height)));
                }
            } else {
                self.child_node1 = Some(Box::new(Self::new(r.x, r.y, r.width, height)));
                if dh - padding > 0.0 {
                    self.child_node2 = Some(Box::new(Self::new(r.x, r.y + height + padding, r.width, dh - padding)));
                }
            }
            return self.child_node1.as_deref_mut()?.find(width, height, padding);
        }
        if let Some(result) = self.child_node1.as_deref_mut().and_then(|c| c.find(width, height, padding)) {
            return Some(result);
        }
        self.child_node2.as_deref_mut().and_then(|c| c.find(width, height, padding))
    }
}

#[derive(Clone, Debug)]
pub struct TexturePacker {
    pub width: f64,
    pub height: f64,
    pub border_padding: f64,
    pub root: TextureNode,
}

impl TexturePacker {
    pub fn new(width: f64, height: f64, border_padding: f64) -> Self {
        Self {
            width,
            height,
            border_padding,
            root: TextureNode::new(
                border_padding,
                border_padding,
                width - 2.0 * border_padding,
                height - 2.0 * border_padding,
            ),
        }
    }

    pub fn pack(&mut self, index: usize, width: f64, height: f64) -> Option<&TextureNode> {
        if !self.width.is_finite()
            || !self.height.is_finite()
            || !self.border_padding.is_finite()
            || self.border_padding < 0.0
        {
            return None;
        }
        if !width.is_finite() || !height.is_finite() || width < 1.0 || height < 1.0 {
            return None;
        }
        let node = self.root.find(width, height, self.border_padding)?;
        node.index = Some(index);
        Some(node)
    }
}
