use crate::graph::Graph;

pub struct SimplifiedPoint {
    pub lon: f64,
    pub lat: f64,
}

impl Graph {
    pub fn simplify_shape(&self, nodes: &[i64], epsilon: f64) -> Vec<SimplifiedPoint> {
        let node_data: Vec<_> = nodes.iter().filter_map(|&id| self.nodes.get(&id)).collect();

        if node_data.len() <= 2 {
            return node_data
                .iter()
                .map(|node| SimplifiedPoint {
                    lon: node.lon,
                    lat: node.lat,
                })
                .collect();
        }

        let points: Vec<(f64, f64)> = node_data.iter().map(|node| (node.lon, node.lat)).collect();

        let simplified = rdp_simplify(&points, epsilon);

        simplified
            .iter()
            .map(|&(lon, lat)| SimplifiedPoint { lon, lat })
            .collect()
    }
}

fn rdp_simplify(points: &[(f64, f64)], epsilon: f64) -> Vec<(f64, f64)> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    let mut result = Vec::new();
    let (index, distance) = find_furthest_point(points);

    if distance > epsilon {
        let first_half = &points[0..=index];
        let second_half = &points[index..];

        let mut simplified_first = rdp_simplify(first_half, epsilon);
        let simplified_second = rdp_simplify(second_half, epsilon);

        simplified_first.pop();

        result.extend(simplified_first);
        result.extend(simplified_second);
    } else {
        result.push(points[0]);
        result.push(points[points.len() - 1]);
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
