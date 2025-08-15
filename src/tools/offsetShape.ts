import { Location } from "../typings";

function processPointSegment(
    [lon1, lat1]: Location,
    [lon2, lat2]: Location,
    offsetMeters: number
): Location[] {
    const offsetDeg = offsetMeters / ((6371000 * Math.PI) / 180.0);
    const avgLatRad = (((lat1 + lat2) / 2.0) * Math.PI) / 180.0;
    const lonFactor = Math.cos(avgLatRad);

    const dx = (lon2 - lon1) * lonFactor;
    const dy = lat2 - lat1;
    const l = Math.sqrt(dx * dx + dy * dy);

    const result: Location[] = [];

    if (l < 1e-10) {
        result.push([lon1, lat1]);
        result.push([lon2, lat2]);
        return result;
    }

    const out1x = lon1 + (offsetDeg * (lat2 - lat1)) / (l * lonFactor);
    const out1y = lat1 + (offsetDeg * (lon1 - lon2)) / l;
    const out2x = lon2 + (offsetDeg * (lat2 - lat1)) / (l * lonFactor);
    const out2y = lat2 + (offsetDeg * (lon1 - lon2)) / l;

    result.push([out1x, out1y]);
    result.push([out2x, out2y]);

    return result;
}

function findIntersection(
    [p1Lon, p1Lat]: Location,
    [p2Lon, p2Lat]: Location,
    [p3Lon, p3Lat]: Location,
    [p4Lon, p4Lat]: Location
): Location | null {
    const a1 = p2Lat - p1Lat;
    const b1 = p1Lon - p2Lon;
    const c1 = a1 * p1Lon + b1 * p1Lat;

    const a2 = p4Lat - p3Lat;
    const b2 = p3Lon - p4Lon;
    const c2 = a2 * p3Lon + b2 * p3Lat;

    const det = a1 * b2 - a2 * b1;

    if (Math.abs(det) < 1e-10) return null;

    return [(b2 * c1 - b1 * c2) / det, (a1 * c2 - a2 * c1) / det];
}

export default (points: Location[], offsetMeters: number = 1.5): Location[] => {
    if (points.length < 2) return [];

    const segments: Location[][] = [];

    for (let i = 0; i < points.length - 1; i++) {
        segments.push(processPointSegment(points[i], points[i + 1], offsetMeters));
    }

    const result: Location[] = [];
    if (segments.length === 0) return result;

    result.push(segments[0][0]);

    for (let i = 0; i < segments.length - 1; i++) {
        const currentSeg = segments[i];
        const nextSeg = segments[i + 1];

        const p1 = points[i];
        const p2 = points[i + 1];
        const p3 = points[i + 2];

        const v1 = [p2[0] - p1[0], p2[1] - p1[1]];
        const v2 = [p3[0] - p2[0], p3[1] - p2[1]];

        const dotProduct = v1[0] * v2[0] + v1[1] * v2[1];

        const mag1 = Math.sqrt(v1[0] * v1[0] + v1[1] * v1[1]);
        const mag2 = Math.sqrt(v2[0] * v2[0] + v2[1] * v2[1]);

        if (mag1 > 0 && mag2 > 0) {
            const normalizedDot = dotProduct / (mag1 * mag2);
            if (normalizedDot >= -0.99) {
                const intersection = findIntersection(currentSeg[0], currentSeg[1], nextSeg[0], nextSeg[1]);

                if (intersection) {
                    result.push(intersection);

                    continue;
                }
            }
        }

        result.push(currentSeg[1]);
        result.push(nextSeg[0]);
    }

    const lastSegment = segments[segments.length - 1];
    result.push(lastSegment[1]);

    return result;
};
