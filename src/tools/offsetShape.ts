import { Location } from "../typings";

function processPointSegment(
    [lon1, lat1]: Location,
    [lon2, lat2]: Location,
    offsetMeters: number
): Location[] {
    const offsetDeg = offsetMeters / ((6371000 * Math.PI) / 180.0);

    const result: Location[] = [];

    const avgLatRad = (((lat1 + lat2) / 2.0) * Math.PI) / 180.0;
    const lonFactor = Math.cos(avgLatRad);

    const dx = (lon2 - lon1) * lonFactor;
    const dy = lat2 - lat1;
    const l = Math.sqrt(dx * dx + dy * dy);

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

export default (points: Location[], offsetMeters: number, offsetSide: number): Location[] => {
    if (points.length < 2) {
        return [];
    }

    const pointsCount = points.length;
    const segments: Location[][] = [];

    for (let i = 0; i < pointsCount - 1; i++) {
        segments.push(processPointSegment(points[i], points[i + 1], offsetMeters * offsetSide));
    }

    const result: Location[] = [];
    if (segments.length === 0) return result;

    result.push(segments[0][0]);

    for (let i = 0; i < segments.length - 1; i++) {
        const currentSeg = segments[i];
        const nextSeg = segments[i + 1];

        const intersection = findIntersection(currentSeg[0], currentSeg[1], nextSeg[0], nextSeg[1]);

        if (intersection) {
            result.push(intersection);
        } else {
            result.push(currentSeg[1]);
            result.push(nextSeg[0]);
        }
    }

    const lastSegment = segments[segments.length - 1];
    result.push(lastSegment[1]);

    return result;
};
