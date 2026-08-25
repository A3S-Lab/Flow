use std::cmp::Reverse;
use std::collections::BinaryHeap;
use wasm_bindgen::prelude::*;

const COLUMN_START_X: f32 = 88.0;
const ROW_START_Y: f32 = 124.0;
const COLUMN_GAP: f32 = 112.0;
const ROW_GAP: f32 = 74.0;
const MINIMUM_NODE_WIDTH: f32 = 240.0;

fn layout(
    node_count: usize,
    sources: &[u32],
    targets: &[u32],
    widths: &[f32],
    heights: &[f32],
) -> Vec<f32> {
    if node_count == 0 || widths.len() != node_count || heights.len() != node_count {
        return Vec::new();
    }

    let mut outgoing = vec![Vec::<usize>::new(); node_count];
    let mut indegree = vec![0_u32; node_count];
    for (&source, &target) in sources.iter().zip(targets) {
        let source = source as usize;
        let target = target as usize;
        if source >= node_count || target >= node_count {
            continue;
        }
        outgoing[source].push(target);
        indegree[target] = indegree[target].saturating_add(1);
    }

    let mut ready = BinaryHeap::new();
    for (index, count) in indegree.iter().enumerate() {
        if *count == 0 {
            ready.push(Reverse(index));
        }
    }

    let mut depths = vec![0_usize; node_count];
    let mut visited = vec![false; node_count];
    while let Some(Reverse(current)) = ready.pop() {
        visited[current] = true;
        for &target in &outgoing[current] {
            depths[target] = depths[target].max(depths[current].saturating_add(1));
            indegree[target] = indegree[target].saturating_sub(1);
            if indegree[target] == 0 {
                ready.push(Reverse(target));
            }
        }
    }

    let fallback_depth = depths.iter().copied().max().unwrap_or(0);
    for (index, was_visited) in visited.into_iter().enumerate() {
        if !was_visited {
            depths[index] = fallback_depth;
        }
    }

    let column_count = depths.iter().copied().max().unwrap_or(0) + 1;
    let mut columns = vec![Vec::<usize>::new(); column_count];
    for (index, depth) in depths.into_iter().enumerate() {
        columns[depth].push(index);
    }

    let mut positions = vec![0.0; node_count * 2];
    let mut column_x = COLUMN_START_X;
    for column in columns {
        let mut row_y = ROW_START_Y;
        let mut column_width = MINIMUM_NODE_WIDTH;
        for index in column {
            positions[index * 2] = column_x;
            positions[index * 2 + 1] = row_y;
            row_y += heights[index].max(0.0) + ROW_GAP;
            column_width = column_width.max(widths[index].max(0.0));
        }
        column_x += column_width + COLUMN_GAP;
    }
    positions
}

#[wasm_bindgen]
pub fn layout_dag(
    node_count: u32,
    sources: &[u32],
    targets: &[u32],
    widths: &[f32],
    heights: &[f32],
) -> Vec<f32> {
    layout(node_count as usize, sources, targets, widths, heights)
}

#[cfg(test)]
mod tests {
    use super::layout;

    #[test]
    fn lays_out_a_chain_in_dependency_columns() {
        let positions = layout(
            3,
            &[0, 1],
            &[1, 2],
            &[240.0, 260.0, 240.0],
            &[126.0, 140.0, 126.0],
        );

        assert_eq!(positions, vec![88.0, 124.0, 440.0, 124.0, 812.0, 124.0]);
    }

    #[test]
    fn preserves_visual_order_within_a_fan_out_column() {
        let positions = layout(4, &[0, 0, 0], &[1, 2, 3], &[240.0; 4], &[126.0; 4]);

        assert_eq!(positions[0..2], [88.0, 124.0]);
        assert_eq!(positions[2..4], [440.0, 124.0]);
        assert_eq!(positions[4..6], [440.0, 324.0]);
        assert_eq!(positions[6..8], [440.0, 524.0]);
    }

    #[test]
    fn rejects_incomplete_numeric_input() {
        assert!(layout(2, &[], &[], &[240.0], &[126.0, 126.0]).is_empty());
    }
}
