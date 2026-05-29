#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub min: Point,
    pub max: Point,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            min: Point { x, y },
            max: Point { x: x + width, y: y + height },
        }
    }

    pub fn width(&self) -> f32  { self.max.x - self.min.x }
    pub fn height(&self) -> f32 { self.max.y - self.min.y }
}

/// PDF 当前变换矩阵（CTM），存储为 `[a, b, c, d, e, f]`。
/// 变换规则：x' = a·x + c·y + e，y' = b·x + d·y + f
#[derive(Debug, Clone, Copy)]
pub struct Matrix(pub [f32; 6]);

impl Matrix {
    pub fn identity() -> Self {
        Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
    }

    pub fn transform_point(&self, p: Point) -> Point {
        let [a, b, c, d, e, f] = self.0;
        Point {
            x: a * p.x + c * p.y + e,
            y: b * p.x + d * p.y + f,
        }
    }

    /// 矩阵串联：先应用 `other`，再应用 `self`（self ∘ other）。
    pub fn concat(&self, other: &Matrix) -> Matrix {
        let [a1, b1, c1, d1, e1, f1] = self.0;
        let [a2, b2, c2, d2, e2, f2] = other.0;
        Matrix([
            a1 * a2 + c1 * b2,
            b1 * a2 + d1 * b2,
            a1 * c2 + c1 * d2,
            b1 * c2 + d1 * d2,
            a1 * e2 + c1 * f2 + e1,
            b1 * e2 + d1 * f2 + f1,
        ])
    }
}
