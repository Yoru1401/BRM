use bevy::math::Vec3;

const LEAF: usize = 4;

struct Node {
    low: Vec3,
    high: Vec3,
    #[allow(dead_code)]
    start: u32,
    count: u32,
    away: u32,
}

pub(crate) struct Bvh {
    nodes: Vec<Node>,
    order: Vec<u32>,
    #[allow(dead_code)]
    always: Vec<u32>,
}

impl Bvh {
    pub(crate) fn build(boxes: &[(Vec3, Vec3)], everywhere: impl Fn(u32) -> bool) -> Self {
        let mut order: Vec<u32> = Vec::with_capacity(boxes.len());
        let mut always: Vec<u32> = Vec::new();
        for index in 0..boxes.len() as u32 {
            match everywhere(index) {
                true => always.push(index),
                false => order.push(index),
            }
        }

        let mut tree = Bvh {
            nodes: Vec::new(),
            order,
            always,
        };
        if !tree.order.is_empty() {
            let span = tree.order.len();
            tree.split(boxes, 0, span);
        }
        tree
    }

    fn split(&mut self, boxes: &[(Vec3, Vec3)], start: usize, count: usize) -> u32 {
        let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for slot in &self.order[start..start + count] {
            let (corner_low, corner_high) = boxes[*slot as usize];
            low = low.min(corner_low);
            high = high.max(corner_high);
        }

        let at = self.nodes.len() as u32;
        self.nodes.push(Node {
            low,
            high,
            start: start as u32,
            count: count as u32,
            away: 0,
        });
        if count <= LEAF {
            return at;
        }

        let reach = high - low;
        let axis = match (reach.x >= reach.y, reach.x >= reach.z, reach.y >= reach.z) {
            (true, true, _) => 0,
            (false, _, true) => 1,
            (_, false, false) => 2,
            _ => 1,
        };
        let middle = count / 2;
        self.order[start..start + count].select_nth_unstable_by(middle, |left, right| {
            let centre = |slot: &u32| {
                let (corner_low, corner_high) = boxes[*slot as usize];
                (corner_low[axis] + corner_high[axis]) * 0.5
            };
            centre(left).total_cmp(&centre(right))
        });

        self.split(boxes, start, middle);
        let away = self.split(boxes, start + middle, count - middle);
        self.nodes[at as usize].count = 0;
        self.nodes[at as usize].away = away;
        at
    }

    pub(crate) fn shallowest(&self, limit: usize) -> Vec<(Vec3, Vec3, u32, bool)> {
        let mut shown = Vec::new();
        if self.nodes.is_empty() {
            return shown;
        }
        let mut queue = std::collections::VecDeque::from([(0u32, 0u32)]);
        while let Some((at, depth)) = queue.pop_front() {
            if shown.len() >= limit {
                break;
            }
            let node = &self.nodes[at as usize];
            shown.push((node.low, node.high, depth, node.count > 0));
            if node.count == 0 {
                queue.push_back((at + 1, depth + 1));
                queue.push_back((node.away, depth + 1));
            }
        }
        shown
    }

    #[allow(dead_code)]
    pub(crate) fn touching(&self, low: Vec3, high: Vec3, found: &mut Vec<u32>) {
        found.clear();
        found.extend_from_slice(&self.always);
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = [0u32; 64];
        let mut depth = 1usize;
        while depth > 0 {
            depth -= 1;
            let at = stack[depth];
            let node = &self.nodes[at as usize];
            if node.low.cmpgt(high).any() || node.high.cmplt(low).any() {
                continue;
            }
            if node.count > 0 {
                let start = node.start as usize;
                found.extend_from_slice(&self.order[start..start + node.count as usize]);
                continue;
            }
            if depth + 2 > stack.len() {
                continue;
            }
            stack[depth] = at + 1;
            stack[depth + 1] = node.away;
            depth += 2;
        }
    }
}
