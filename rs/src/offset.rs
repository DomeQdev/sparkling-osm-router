use crate::graph::{Graph, Node};

const EARTH_RADIUS: f64 = 6371000.0;

#[derive(Clone, Debug)]
pub struct OffsetPoint {
    pub lon: f64,
    pub lat: f64,
}

impl Graph {
    pub fn offset_route_shape(
        &self,
        nodes: &[i64],
        offset_meters: f64,
        offset_side: i8,
    ) -> Vec<OffsetPoint> {
        if nodes.len() < 2 {
            return Vec::new();
        }

        let mut segments = Vec::with_capacity(nodes.len() - 1);
        for i in 0..nodes.len() - 1 {
            if let (Some(node1), Some(node2)) =
                (self.nodes.get(&nodes[i]), self.nodes.get(&nodes[i + 1]))
            {
                segments.push(process_segment(
                    node1,
                    node2,
                    offset_meters * offset_side as f64,
                ));
            }
        }

        let mut result = Vec::with_capacity(nodes.len());

        if !segments.is_empty() {
            if let Some(first_segment) = segments.first() {
                if let Some(first_point) = first_segment.first() {
                    result.push(first_point.clone());
                }
            }

            for i in 0..segments.len() - 1 {
                let segment = &segments[i];
                let next_segment = &segments[i + 1];

                if segment.len() < 2 || next_segment.len() < 2 {
                    continue;
                }

                if let Some(intersection) =
                    find_intersection(&segment[0], &segment[1], &next_segment[0], &next_segment[1])
                {
                    result.push(intersection);
                } else {
                    result.push(segment[1].clone());
                }
            }

            if let Some(last_segment) = segments.last() {
                if let Some(last_point) = last_segment.last() {
                    if result.last().map_or(true, |p| {
                        (p.lon - last_point.lon).abs() > 1e-10
                            || (p.lat - last_point.lat).abs() > 1e-10
                    }) {
                        result.push(last_point.clone());
                    }
                }
            }
        }

        result
    }
}

fn process_segment(point1: &Node, point2: &Node, offset_meters: f64) -> Vec<OffsetPoint> {
    let offset_deg = offset_meters / (EARTH_RADIUS * std::f64::consts::PI / 180.0);

    let lon1 = point1.lon;
    let lat1 = point1.lat;
    let lon2 = point2.lon;
    let lat2 = point2.lat;

    let avg_lat_rad = ((lat1 + lat2) / 2.0).to_radians();
    let lon_factor = avg_lat_rad.cos();

    let dx = (lon2 - lon1) * lon_factor;
    let dy = lat2 - lat1;
    let l = (dx * dx + dy * dy).sqrt();

    if l < 1e-10 {
        return vec![
            OffsetPoint {
                lon: lon1,
                lat: lat1,
            },
            OffsetPoint {
                lon: lon2,
                lat: lat2,
            },
        ];
    }

    let out1x = lon1 + (offset_deg * (lat2 - lat1)) / (l * lon_factor);
    let out1y = lat1 + (offset_deg * (lon1 - lon2)) / l;
    let out2x = lon2 + (offset_deg * (lat2 - lat1)) / (l * lon_factor);
    let out2y = lat2 + (offset_deg * (lon1 - lon2)) / l;

    vec![
        OffsetPoint {
            lon: out1x,
            lat: out1y,
        },
        OffsetPoint {
            lon: out2x,
            lat: out2y,
        },
    ]
}

fn find_intersection(
    p1: &OffsetPoint,
    p2: &OffsetPoint,
    p3: &OffsetPoint,
    p4: &OffsetPoint,
) -> Option<OffsetPoint> {
    let a1 = p2.lat - p1.lat;
    let b1 = p1.lon - p2.lon;
    let c1 = a1 * p1.lon + b1 * p1.lat;

    let a2 = p4.lat - p3.lat;
    let b2 = p3.lon - p4.lon;
    let c2 = a2 * p3.lon + b2 * p3.lat;

    let det = a1 * b2 - a2 * b1;

    if det.abs() < 1e-10 {
        return None;
    }

    let x = (b2 * c1 - b1 * c2) / det;
    let y = (a1 * c2 - a2 * c1) / det;

    let on_segment1 = is_point_on_segment(x, y, p1.lon, p1.lat, p2.lon, p2.lat);
    let on_segment2 = is_point_on_segment(x, y, p3.lon, p3.lat, p4.lon, p4.lat);

    if on_segment1 && on_segment2 {
        Some(OffsetPoint { lon: x, lat: y })
    } else {
        None
    }
}

fn is_point_on_segment(x: f64, y: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> bool {
    let buffer = 1e-10;
    let min_x = x1.min(x2) - buffer;
    let max_x = x1.max(x2) + buffer;
    let min_y = y1.min(y2) - buffer;
    let max_y = y1.max(y2) + buffer;

    x >= min_x && x <= max_x && y >= min_y && y <= max_y
}
