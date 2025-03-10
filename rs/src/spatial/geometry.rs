pub const EARTH_RADIUS: f64 = 6371000.0;

pub fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371.0;
    let lat1_rad = lat1.to_radians();
    let lon1_rad = lon1.to_radians();
    let lat2_rad = lat2.to_radians();
    let lon2_rad = lon2.to_radians();

    let dlat = lat2_rad - lat1_rad;
    let dlon = lon2_rad - lon1_rad;

    let a =
        (dlat / 2.0).sin().powi(2) + lat1_rad.cos() * lat2_rad.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

    r * c
}

pub fn calculate_bearing(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1_rad = lat1.to_radians();
    let lon1_rad = lon1.to_radians();
    let lat2_rad = lat2.to_radians();
    let lon2_rad = lon2.to_radians();

    let dlon = lon2_rad - lon1_rad;

    let y = dlon.sin() * lat2_rad.cos();
    let x = lat1_rad.cos() * lat2_rad.sin() - lat1_rad.sin() * lat2_rad.cos() * dlon.cos();

    let bearing_rad = y.atan2(x);
    let mut bearing_deg = bearing_rad.to_degrees();

    if bearing_deg < 0.0 {
        bearing_deg += 360.0;
    }

    bearing_deg
}

pub fn bearing_difference(bearing1: f64, bearing2: f64) -> f64 {
    let mut diff = bearing2 - bearing1;
    while diff > 180.0 {
        diff -= 360.0;
    }
    while diff < -180.0 {
        diff += 360.0;
    }
    diff
}

pub fn squared_distance(p1: &[f64; 2], p2: &[f64; 2]) -> f64 {
    (p1[0] - p2[0]).powi(2) + (p1[1] - p2[1]).powi(2)
}

pub fn point_to_segment_distance(p: &[f64; 2], a: &[f64; 2], b: &[f64; 2]) -> f64 {
    let ab_x = b[0] - a[0];
    let ab_y = b[1] - a[1];

    if ab_x.abs() < 1e-10 && ab_y.abs() < 1e-10 {
        return squared_distance(p, a);
    }

    let ap_x = p[0] - a[0];
    let ap_y = p[1] - a[1];

    let t = (ap_x * ab_x + ap_y * ab_y) / (ab_x * ab_x + ab_y * ab_y);

    let t_clamped = t.max(0.0).min(1.0);

    let closest_x = a[0] + t_clamped * ab_x;
    let closest_y = a[1] + t_clamped * ab_y;

    (p[0] - closest_x).powi(2) + (p[1] - closest_y).powi(2)
}
