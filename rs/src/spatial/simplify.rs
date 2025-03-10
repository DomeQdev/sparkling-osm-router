use crate::core::types::Graph;

#[derive(Clone, Debug)]
pub struct SimplifiedPoint {
    pub lon: f64,
    pub lat: f64,
}

impl Graph {
    pub fn simplify_shape(&self, nodes: &[i64], epsilon: f64) -> Vec<SimplifiedPoint> {
        let node_count = nodes.len();
        let mut node_data = Vec::with_capacity(node_count);
        
        for &id in nodes {
            if let Some(node) = self.nodes.get(&id) {
                node_data.push((node.lon, node.lat));
            }
        }

        if node_data.len() <= 2 {
            return node_data.into_iter()
                   .map(|(lon, lat)| SimplifiedPoint { lon, lat })
                   .collect();
        }

        let simplified = rdp_simplify(&node_data, epsilon);

        let mut result = Vec::with_capacity(simplified.len());
        for &(lon, lat) in &simplified {
            result.push(SimplifiedPoint { lon, lat });
        }

        result
    }
}

fn rdp_simplify(points: &[(f64, f64)], epsilon: f64) -> Vec<(f64, f64)> {
    let len = points.len();
    
    if len <= 2 {
        return points.to_vec();
    }

    let mut result = Vec::with_capacity(len / 2 + 2);
    let (index, distance) = find_furthest_point(points);

    if distance > epsilon {
        let mut simplified_first = rdp_simplify(&points[0..=index], epsilon);
        let simplified_second = rdp_simplify(&points[index..], epsilon);

        simplified_first.pop();

        result.reserve(simplified_first.len() + simplified_second.len());
        result.append(&mut simplified_first);
        result.extend_from_slice(&simplified_second);
    } else {
        result.push(points[0]);
        result.push(points[len - 1]);
    }

    result
}

fn find_furthest_point(points: &[(f64, f64)]) -> (usize, f64) {
    if points.len() <= 2 {
        return (0, 0.0);
    }

    let start = points[0];
    let end = points[points.len() - 1];

    let mut max_distance = 0.0;
    let mut max_index = 0;

    for (i, &point) in points.iter().enumerate().skip(1).take(points.len() - 2) {
        let distance = perpendicular_distance(point, start, end);

        if distance > max_distance {
            max_distance = distance;
            max_index = i;
        }
    }

    (max_index, max_distance)
}

fn perpendicular_distance(point: (f64, f64), line_start: (f64, f64), line_end: (f64, f64)) -> f64 {
    let (x, y) = point;
    let (x1, y1) = line_start;
    let (x2, y2) = line_end;

    let line_length = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();

    if line_length == 0.0 {
        return ((x - x1).powi(2) + (y - y1).powi(2)).sqrt();
    }

    let area = ((x2 - x1) * (y1 - y) - (x1 - x) * (y2 - y1)).abs();
    area / line_length
}